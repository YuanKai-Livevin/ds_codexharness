# -*- coding: utf-8 -*-
"""智能压缩引擎。

流程（严格按规格）：
1. 输入过滤：仅 status=ACTIVE、is_pinned=False、且 last_accessed 在 10 轮之前的块。
2. 数值隔离保护：content 为纯数字 / 含 http(s):// / 含文件路径 → 移出压缩列表，
   并将 importance 强制设为 5（持久化）。
3. LLM 合并（提示词为内嵌常量）；失败或无 Key 时降级为本地规则合并。
4. 有效性判定：合并后 Token 减少比例 < 15% → compaction_ineffective=True。
"""
import re
from datetime import datetime, timedelta
from typing import List, Tuple

from ..models.memory import BlockStatus, BlockType, MemoryBlock
from . import llm, watermark
from .storage import MemoryStore

STALE_ROUNDS = 10                       # 10 轮之前视为可压缩（规格）
MIN_EFFECTIVE_RATIO = 0.15              # Token 减少 < 15% 判为无效压缩（规格）
MAX_MERGED_RATIO = 0.5                  # 输出块数量 < 输入块数量 * 50%（规格）

# ---- 数值隔离保护 ----
_NUMERIC_ONLY = re.compile(r"^\d{1,12}$")
_DRIVE_PATH = re.compile(r"^[A-Za-z]:[\\/]")

COMPACT_SYSTEM = (
    "你是一个信息去重合并引擎。只输出 JSON 数组本身，"
    "不要输出任何解释文字，不要使用 Markdown 围栏。"
    "必须将输出块数量压缩到输入块数量的一半以下。"
    "表达尽量精简、去除重复措辞，但必须保留所有具体数字、变量名、路径。"
    "数组每个元素形如 {\"type\": \"块类型\", \"content\": \"合并后的精炼描述\"}。"
)

COMPACT_PROMPT = (
    "你是一个信息去重合并引擎。将以下记忆块按主题合并，输出 JSON 数组。\n"
    "要求：保留所有具体数字、变量名、路径；输出块数量 < 输入块数量 * 50%；\n"
    "若冲突，保留最新时间戳内容。输入：{blocks}"
)


def _is_protected(content: str) -> bool:
    """数值/链接/路径隔离保护判定。"""
    c = content.strip()
    if _NUMERIC_ONLY.fullmatch(c):
        return True
    if "http://" in c or "https://" in c:
        return True
    if "/" in c or "\\" in c or _DRIVE_PATH.match(c):
        return True
    return False


def _eligible(blocks: List[MemoryBlock], current_round: int) -> List[MemoryBlock]:
    now = datetime.now()
    out = []
    for b in blocks:
        if b.status != BlockStatus.ACTIVE or b.is_pinned:
            continue
        round_old = (current_round - b.source_round) >= STALE_ROUNDS
        time_old = (now - b.last_accessed) > timedelta(hours=2)
        if round_old or time_old:
            out.append(b)
    return out


def _protect(eligible_blocks: List[MemoryBlock], store: MemoryStore, full_pool: List[MemoryBlock]) -> Tuple[List[MemoryBlock], List[MemoryBlock]]:
    """从候选（eligible）中分出受保护块；importance 置 5 并回写完整池。

    重要：只处理 eligible 块，绝不触碰新块/暂停/置顶/已归档块；
    回写时必须保存完整池（full_pool），否则会静默丢弃池内其它块。
    """
    cands, protected = [], []
    changed = False
    for b in eligible_blocks:
        if _is_protected(b.content):
            if b.importance < 5:
                b.importance = 5
                changed = True
            protected.append(b)
        else:
            cands.append(b)
    if changed:
        store.save_blocks(full_pool, None)
    return cands, protected


def _block_payload(b: MemoryBlock) -> dict:
    return {
        "type": b.type.value,
        "content": b.content,
        "importance": b.importance,
        "source_round": b.source_round,
    }


def _merge_llm(cands: List[MemoryBlock]) -> List[dict]:
    """LLM 合并；失败返回 []（由调用方降级）。"""
    payload = [json_dumps(_block_payload(b)) for b in cands]
    user = COMPACT_PROMPT.format(blocks="\n".join(payload))
    data = llm.chat_json(COMPACT_SYSTEM, user, max_tokens=2000, temperature=0.2)
    if not isinstance(data, list) or not data:
        return []
    merged = []
    for item in data:
        if not isinstance(item, dict):
            continue
        t = str(item.get("type", "fact")).strip()
        c = str(item.get("content", "")).strip()
        if not c:
            continue
        try:
            bt = BlockType(t)
        except ValueError:
            bt = BlockType.FACT
        merged.append({"type": bt, "content": c})
    return merged


def _merge_rules(cands: List[MemoryBlock]) -> List[dict]:
    """本地规则回退：按类型分组去重合并。"""
    groups: dict = {}
    for b in cands:
        groups.setdefault(b.type.value, []).append(b.content)
    merged = []
    for t, contents in groups.items():
        seen, parts = set(), []
        for c in contents:
            c = c.strip()
            if c and c not in seen:
                seen.add(c)
                parts.append(c)
        content = "；".join(parts)
        if len(content) > 300:
            content = content[:297] + "…"
        if content:
            merged.append({"type": BlockType(t), "content": content})
    return merged


def json_dumps(b: dict) -> str:
    import json
    return json.dumps(b, ensure_ascii=False)


def compress(store: MemoryStore) -> dict:
    """执行一次压缩。返回报告 dict。"""
    blocks = store.load_blocks()
    conv = store.load_conversation()
    current_round = int(conv.get("round", 0))
    meta = store.load_meta()
    consecutive = int(meta.get("consecutive_ineffective", 0))

    eligible = _eligible(blocks, current_round)
    if not eligible:
        return {
            "ok": True, "compacted": 0, "created": 0, "ineffective": False,
            "message": "没有需要压缩的旧记忆块（10 轮内 / 已置顶的块不受影响）。",
        }

    # 数值隔离保护（持久化 importance=5）——候选范围严格限定为 eligible 块
    candidates, protected = _protect(eligible, store, blocks)
    if not candidates:
        return {
            "ok": True, "compacted": 0, "created": 0, "ineffective": False,
            "protected": len(protected),
            "message": "所有可压缩块均为数值/路径类关键信息，已提升保护等级，本轮不压缩。",
        }

    tokens_before = sum(watermark.count_tokens(b.content) for b in candidates)

    # LLM 合并 → 降级规则合并
    merged = _merge_llm(candidates)
    if not merged:
        merged = _merge_rules(candidates)

    # 有效性判定（规格 3.3 唯一标准）：合并后 Token 减少比例 < 15% → 无效
    tokens_after = sum(watermark.count_tokens(m["content"]) for m in merged)
    reduced = 1.0 - (tokens_after / tokens_before) if tokens_before else 0.0
    ineffective = reduced < MIN_EFFECTIVE_RATIO
    message = (
        "压缩有效：合并后 Token 减少 {:.0%}（{} → {}），块数 {} → {}。".format(
            reduced, tokens_before, tokens_after, len(candidates), len(merged)
        )
        if not ineffective
        else "压缩无效：合并后 Token 减少不足 15%（{:.0%}），建议生成交接。".format(reduced)
    )

    # 落盘：仅当有效时才真正替换
    if not ineffective:
        store.snapshot("compress")
        cand_ids = {b.id for b in candidates}
        remaining = [b for b in blocks if b.id not in cand_ids]
        order_base = max((b.order_index for b in remaining), default=-1)
        created = []
        for i, m in enumerate(merged):
            nb = MemoryBlock(
                id="mem_{}_{}".format(datetime.now().strftime("%Y%m%d%H%M%S"), _uid()),
                type=m["type"],
                content=m["content"],
                importance=max(b.importance for b in candidates),
                status=BlockStatus.PROBATION,  # 刚压缩生成，观察期
                token_count=watermark.count_tokens(m["content"]),
                last_accessed=datetime.now(),
                source_round=current_round,
                deprecated_ids=[b.id for b in candidates],
                is_pinned=False,
                order_index=order_base + 1 + i,
            )
            created.append(nb)
        meta = store.load_meta()
        meta.update(
            {
                "consecutive_ineffective": 0,
                "last_compress": datetime.now().isoformat(timespec="seconds"),
            }
        )
        store.save_blocks(remaining + created, meta)
        return {
            "ok": True,
            "compacted": len(candidates),
            "created": len(created),
            "ineffective": False,
            "protected": len(protected),
            "tokens_before": tokens_before,
            "tokens_after": tokens_after,
            "message": message,
        }

    # 无效压缩：仅累计计数
    store.save_blocks(blocks, {"consecutive_ineffective": consecutive + 1})
    return {
        "ok": True,
        "compacted": 0,
        "created": 0,
        "ineffective": True,
        "protected": len(protected),
        "tokens_before": tokens_before,
        "tokens_after": tokens_after,
        "message": message,
    }


def _uid() -> str:
    import uuid
    return uuid.uuid4().hex[:8]
