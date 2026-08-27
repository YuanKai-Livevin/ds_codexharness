# -*- coding: utf-8 -*-
"""阶段总结服务。

- 用户按阶段工作：一个阶段做完后，手动「生成阶段总结」。
- 预览：LLM 基于当前活动记忆块 + 阶段说明生成 Markdown 总结。
- 确认：旧记忆块自动归档（ACTIVE/PROBATION → DEPRECATED，置顶保留），
  生成 1 个「阶段总结」记忆块（type=phase，受保护），并追加到 phases.json 记录。
- 全部过程保留 30 分钟快照，可紧急回滚。
"""
import json
import re
from datetime import datetime
from typing import List, Optional

from ..models.memory import BlockStatus, BlockType, MemoryBlock
from . import llm, watermark
from .storage import MemoryStore

PHASE_MAX_CHARS = 800   # 阶段总结 ≤ 800 字（比交接更轻量）

PHASE_SYSTEM = (
    "你是项目阶段总结助手。基于记忆块生成简洁的阶段总结 Markdown。"
    "只输出 Markdown 本身，不要解释。输出总字数不超过 800 字。"
)

PHASE_PROMPT = (
    "基于以下记忆块，为用户当前的工作阶段生成阶段总结。\n"
    "严格按此结构输出 Markdown：\n"
    "## 阶段目标\n"
    "## 已完成\n"
    "## 关键产出（保留具体数字、文件名、路径）\n"
    "## 遗留事项\n"
    "## 下一步计划\n"
    "阶段说明：{goal}\n"
    "记忆块：\n{blocks}"
)


def _active_blocks(store: MemoryStore) -> List[MemoryBlock]:
    blocks = store.load_blocks()
    actives = [b for b in blocks if b.status in (BlockStatus.ACTIVE, BlockStatus.PROBATION)]
    actives.sort(key=lambda b: (-b.importance, b.order_index))
    return actives


def _fallback_summary(blocks: List[MemoryBlock], goal: str) -> str:
    lines = ["## 阶段目标", "", "- " + (goal or "（未记录阶段说明）"), ""]
    done = [b for b in blocks if b.type not in (BlockType.TASK, BlockType.PLAN)]
    if done:
        lines += ["## 已完成", ""]
        for b in done[:8]:
            lines.append("- " + b.content[:60])
    else:
        lines += ["## 已完成", "", "- 暂无记录", ""]
    assets = [b for b in blocks if b.importance >= 4 and b.type in (BlockType.CODE_SNIPPET, BlockType.FACT)]
    lines += ["", "## 关键产出（保留具体数字、文件名、路径）", ""]
    if assets:
        for b in assets[:6]:
            lines.append("- " + b.content)
    else:
        lines.append("- 暂无高价值产出")
    lines += ["", "## 遗留事项", "", "- 待补充", "", "## 下一步计划", "", "- 待补充"]
    return "\n".join(lines)


def preview(store: MemoryStore, goal: str = "") -> dict:
    """生成阶段总结（预览，不落盘）。"""
    actives = _active_blocks(store)
    payload = "\n".join(
        "[{}][imp={}] {}".format(b.type.value, b.importance, b.content[:100]) for b in actives[:40]
    )
    md = llm.chat(
        PHASE_SYSTEM,
        PHASE_PROMPT.format(goal=goal or "（未记录）", blocks=payload),
        max_tokens=1200,
        temperature=0.4,
        timeout=25,     # 限时 25 秒：超时立即用规则模板兜底，避免「正在生成」久等
        retries=1,
    )
    if not md:
        md = _fallback_summary(actives, goal)
    md = md.strip()
    if len(md) > PHASE_MAX_CHARS:
        md = md[:PHASE_MAX_CHARS]
    return {
        "ok": True,
        "summary": md,
        "chars": len(md),
        "tokens": watermark.count_tokens(md),
        "blocks_used": len(actives),
    }


def confirm(store: MemoryStore, goal: str = "", summary: Optional[str] = None,
            open_new_thread: bool = False) -> dict:
    """确认阶段总结：归档旧块 → 生成阶段块 → 记录 phases.json。"""
    if not summary or not summary.strip():
        summary = preview(store, goal)["summary"]
    summary = summary.strip()
    store.snapshot("phase")

    archived = store.archive_active_blocks(keep_pinned=True)

    # 阶段总结记忆块（短摘要，受保护，进入新阶段上下文）
    short = _shorten(summary)
    now = datetime.now()
    conv = store.load_conversation()
    cur_round = int(conv.get("round", 0))
    blocks = store.load_blocks()
    order = max((b.order_index for b in blocks), default=-1) + 1
    phase_block = MemoryBlock(
        id="mem_{}_{}".format(now.strftime("%Y%m%d%H%M%S"), _uid()),
        type=BlockType.PHASE,
        content=short,
        importance=5,
        status=BlockStatus.ACTIVE,
        token_count=watermark.count_tokens(short),
        last_accessed=now,
        source_round=cur_round,
        is_pinned=False,
        order_index=order,
    )
    store.save_blocks(blocks + [phase_block])

    entry = {
        "id": phase_block.id,
        "goal": goal or "",
        "summary": summary,
        "created_at": now.isoformat(timespec="seconds"),
        "archived": len(archived),
        "archived_ids": [b.id for b in archived],
        "open_new_thread": open_new_thread,
    }
    store.append_phase(entry)

    meta = store.load_meta()
    meta["last_phase"] = now.isoformat(timespec="seconds")
    store.save_blocks(store.load_blocks(), meta)

    return {
        "ok": True,
        "phase_id": phase_block.id,
        "archived": len(archived),
        "summary": summary,
        "message": "阶段已归档：关闭 {} 个记忆块，生成阶段总结。".format(len(archived)),
    }


def list_phases(store: MemoryStore) -> list:
    return store.load_phases()


def _shorten(summary: str) -> str:
    """从总结中提取 ≤50 字的核心摘要作为记忆块内容。"""
    text = re.sub(r"^#+\s*", "", summary)
    text = re.sub(r"[#*`>-]", "", text)
    text = " ".join(text.split())
    if not text:
        return "阶段总结"
    if len(text) > 48:
        text = text[:47] + "…"
    return text


def _uid() -> str:
    import uuid
    return uuid.uuid4().hex[:8]
