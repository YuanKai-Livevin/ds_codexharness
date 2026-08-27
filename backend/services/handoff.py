# -*- coding: utf-8 -*-
"""交接文档生成器。

- 触发条件（纯函数判定）：连续 2 次压缩无效，或活跃块 > 15 且类型超过 4 种。
- LLM 按规格提示词生成 Markdown（总字数 ≤ 1500，Token ≤ 2000）。
- 确认交接：快照旧池（30 分钟回滚窗口）→ 清空 → 生成 3 个种子块（约束/计划/事实）。
"""
import re
from datetime import datetime
from typing import List

from ..models.memory import BlockStatus, BlockType, MemoryBlock
from . import llm, watermark
from .storage import MemoryStore

HANDOFF_MAX_CHARS = 1500   # 规格：总字数 ≤ 1500
HANDOFF_MAX_TOKENS = 2000  # 交付清单：≤ 2K Tokens

HANDOFF_SYSTEM = (
    "你是项目交接官。严格按给定结构输出 Markdown，不输出任何额外解释。"
    "输出总字数必须不超过 1500 字。"
)

HANDOFF_PROMPT = (
    "你是项目交接官。基于全部记忆块和原始目标，生成交接清单。\n"
    "严格按此结构输出 Markdown（总字数 ≤ 1500）：\n"
    "# 任务宪法（目标+约束）\n"
    "# 当前进度（已完成/进行中/待办）\n"
    "# 关键资产（仅代码段和数字）\n"
    "# 遗留风险与用户语气样本\n"
    "原始目标：{goal}\n"
    "记忆块：\n{blocks}"
)


def needs_handoff(store: MemoryStore) -> bool:
    """交接触发条件（纯函数，不依赖 LLM）。"""
    blocks = store.load_blocks()
    meta = store.load_meta()
    if int(meta.get("consecutive_ineffective", 0)) >= 2:
        return True
    active = [b for b in blocks if b.status == BlockStatus.ACTIVE]
    if len(active) > 15:
        types = {b.type for b in active}
        if len(types) > 4:
            return True
    return False


def _render_fallback(blocks: List[MemoryBlock], goal: str) -> str:
    """无 LLM 时的模板式交接文档。"""
    lines = ["# 任务宪法（目标+约束）", "", "- 目标：" + (goal or "（未记录）"), ""]
    constraints = [b for b in blocks if b.type == BlockType.CONSTRAINT]
    if constraints:
        lines.append("- 约束：")
        for b in constraints[:5]:
            lines.append("  - " + b.content)
    lines += ["", "# 当前进度（已完成/进行中/待办）", ""]
    lines.append("- 进行中：暂无记录（请补充）")
    lines += ["", "# 关键资产（仅代码段和数字）", ""]
    assets = [b for b in blocks if b.type in (BlockType.CODE_SNIPPET, BlockType.FACT) and b.importance >= 4]
    if assets:
        for b in assets[:8]:
            lines.append("- " + b.content)
    else:
        lines.append("- 暂无高价值资产")
    lines += ["", "# 遗留风险与用户语气样本", ""]
    risks = [b for b in blocks if b.type == BlockType.CONSTRAINT and b.importance == 5]
    lines.append("- 待评估（压缩无效时建议人工确认）" if risks else "- 暂无重大遗留风险")
    return "\n".join(lines)


def generate_markdown(store: MemoryStore, goal: str = "") -> str:
    """生成交接 Markdown（LLM 优先，规则回退），并强制长度约束。"""
    blocks = store.load_blocks()
    actives = [b for b in blocks if b.status in (BlockStatus.ACTIVE, BlockStatus.PROBATION)]
    actives.sort(key=lambda b: (-b.importance, b.order_index))

    payload = "\n".join(
        "[{}][imp={}] {}".format(b.type.value, b.importance, b.content[:120]) for b in actives[:60]
    )
    md = llm.chat(HANDOFF_SYSTEM, HANDOFF_PROMPT.format(goal=goal or "（未记录）", blocks=payload), max_tokens=2500, temperature=0.4)
    if not md:
        md = _render_fallback(actives, goal)
    md = _enforce_limits(md)
    return md


def _enforce_limits(md: str) -> str:
    """确保 ≤1500 字且 ≤2000 tokens：超限则按 token 截断到安全长度。"""
    if len(md) > HANDOFF_MAX_CHARS:
        md = md[:HANDOFF_MAX_CHARS]
    toks = watermark.count_tokens(md)
    if toks > HANDOFF_MAX_TOKENS:
        # 逐段截断
        ratio = HANDOFF_MAX_TOKENS / toks
        md = md[: max(200, int(len(md) * ratio * 0.9))]
    return md.strip()


def _section_lines(md: str, title: str) -> List[str]:
    """从 Markdown 中提取某小节下的子弹行。"""
    out: List[str] = []
    lines = md.splitlines()
    idx = None
    for i, ln in enumerate(lines):
        if ln.strip().startswith("#") and title in ln:
            idx = i
            break
    if idx is None:
        return out
    for ln in lines[idx + 1:]:
        s = ln.strip()
        if s.startswith("#") and not s.startswith("##"):
            break
        if s.startswith("-") or s.startswith("*"):
            item = s.lstrip("-* ").strip()
            if item:
                out.append(item)
    return out


def _seed_content(store: MemoryStore, goal: str) -> List[MemoryBlock]:
    """基于交接文档生成 3 个种子块：约束、计划、事实。"""
    md = generate_markdown(store, goal)
    now = datetime.now()
    conv = store.load_conversation()
    cur_round = int(conv.get("round", 0))

    constraints = _section_lines(md, "任务宪法") or ["目标：" + (goal or "延续当前任务")]
    plans = _section_lines(md, "当前进度") or ["继续当前任务并定期汇总进度"]
    facts = _section_lines(md, "关键资产") or ["工作区与任务上下文已交接"]

    def make(bt: BlockType, content: str) -> MemoryBlock:
        content = content[:50]  # 建议 ≤ 50 字
        return MemoryBlock(
            id="mem_{}_{}".format(now.strftime("%Y%m%d%H%M%S"), _uid()),
            type=bt,
            content=content,
            importance=5 if bt == BlockType.CONSTRAINT else 4,
            status=BlockStatus.ACTIVE,
            token_count=watermark.count_tokens(content),
            last_accessed=now,
            source_round=cur_round,
            is_pinned=False,
            order_index=0,
        )

    seeds = [
        make(BlockType.CONSTRAINT, constraints[0]),
        make(BlockType.PLAN, plans[0] if plans else "继续当前任务并定期汇总进度"),
        make(BlockType.FACT, facts[0] if facts else "上下文已交接"),
    ]
    for i, s in enumerate(seeds):
        s.order_index = i
    return seeds


def confirm(store: MemoryStore, goal: str = "") -> dict:
    """确认交接：快照旧池 → 清空 → 3 个种子块。"""
    blocks = store.load_blocks()
    snap = store.snapshot("handoff") if blocks else None
    seeds = _seed_content(store, goal)
    store.save_blocks(
        seeds,
        {
            "consecutive_ineffective": 0,
            "last_handoff": datetime.now().isoformat(timespec="seconds"),
        },
    )
    return {
        "ok": True,
        "snapshot": snap,
        "seeds": [s.model_dump(mode="json") for s in seeds],
        "old_count": len(blocks),
        "message": "交接完成：旧记忆已暂存（30 分钟内可紧急回滚），已生成 3 个种子块。",
    }


def _uid() -> str:
    import uuid
    return uuid.uuid4().hex[:8]
