# -*- coding: utf-8 -*-
"""FastAPI 路由：记忆块 CRUD + 压缩 + 交接 + 水位。

端点（规格 + 面板必需扩展）：
  GET    /api/memory/blocks            全部块（按 order_index 排序）
  POST   /api/memory/blocks            手动添加
  PATCH  /api/memory/blocks/order      批量排序（先于 {id} 声明）
  PATCH  /api/memory/blocks/{id}       更新 status/content/is_pinned
  DELETE /api/memory/blocks/{id}       删除
  POST   /api/memory/compress          手动触发压缩
  POST   /api/memory/handoff/preview   生成交接文档预览（不落盘）
  POST   /api/memory/handoff/confirm   确认交接（重置为 3 种子块）
  POST   /api/memory/handoff/rollback  紧急回滚（隐藏接口）
  GET    /api/memory/status            水位/计数/阈值
  POST   /api/memory/usage             推送真实对话 tokens
"""
import os
from datetime import datetime

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field
from typing import List, Optional

from ..models.memory import BlockStatus, BlockType, MemoryBlock
from ..services import compactor, handoff, phase, watermark
from ..services.storage import MemoryStore

router = APIRouter(prefix="/memory", tags=["memory"])


# ---------- 存储单例（数据目录来自环境变量，应用启动时注入） ----------
_store: Optional[MemoryStore] = None


def get_store() -> MemoryStore:
    global _store
    if _store is None:
        data_dir = os.environ.get("HARNESS_DATA_DIR") or os.path.join(os.getcwd(), "data")
        _store = MemoryStore(data_dir)
    return _store


def _uid() -> str:
    import uuid
    return uuid.uuid4().hex[:8]


# ---------- 请求体模型 ----------
class BlockCreate(BaseModel):
    type: BlockType = BlockType.FACT
    content: str = Field(..., min_length=1, max_length=500)
    importance: int = Field(3, ge=1, le=5)
    is_pinned: bool = False


class BlockPatch(BaseModel):
    status: Optional[BlockStatus] = None
    content: Optional[str] = None
    is_pinned: Optional[bool] = None
    type: Optional[BlockType] = None
    importance: Optional[int] = Field(None, ge=1, le=5)


class OrderItem(BaseModel):
    id: str
    order_index: int


class OrderPatch(BaseModel):
    items: List[OrderItem]


class UsagePush(BaseModel):
    tokens: int = Field(0, ge=0)
    round_inc: int = Field(1, ge=0, le=10)


class GoalBody(BaseModel):
    goal: Optional[str] = None


class PhaseConfirmBody(BaseModel):
    goal: Optional[str] = None
    summary: Optional[str] = None
    open_new_thread: bool = False


# ---------- 记忆块 CRUD ----------
@router.get("/blocks")
def list_blocks():
    blocks = get_store().load_blocks()
    blocks.sort(key=lambda b: (b.order_index, b.last_accessed.isoformat()))
    return {"ok": True, "blocks": [b.model_dump(mode="json") for b in blocks]}


@router.post("/blocks")
def create_block(body: BlockCreate):
    store = get_store()
    conv = store.load_conversation()
    cur_round = int(conv.get("round", 0))
    blocks = store.load_blocks()
    order = max((b.order_index for b in blocks), default=-1) + 1
    nb = MemoryBlock(
        id="mem_{}_{}".format(datetime.now().strftime("%Y%m%d%H%M%S"), _uid()),
        type=body.type,
        content=body.content.strip(),
        importance=body.importance,
        status=BlockStatus.ACTIVE,
        token_count=watermark.count_tokens(body.content),
        last_accessed=datetime.now(),
        source_round=cur_round,
        is_pinned=body.is_pinned,
        order_index=order,
    )
    store.save_blocks(blocks + [nb])
    return {"ok": True, "block": nb.model_dump(mode="json")}


@router.patch("/blocks/order")
def reorder(body: OrderPatch):
    store = get_store()
    blocks = store.load_blocks()
    by_id = {b.id: b for b in blocks}
    for item in body.items:
        if item.id in by_id:
            by_id[item.id].order_index = item.order_index
    store.save_blocks(blocks)
    return {"ok": True, "count": len(body.items)}


@router.patch("/blocks/{block_id}")
def patch_block(block_id: str, body: BlockPatch):
    store = get_store()
    blocks = store.load_blocks()
    for b in blocks:
        if b.id == block_id:
            if body.status is not None:
                b.status = body.status
            if body.content is not None:
                b.content = body.content.strip()
                b.token_count = watermark.count_tokens(b.content)
            if body.type is not None:
                b.type = body.type
            if body.importance is not None:
                b.importance = body.importance
            if body.is_pinned is not None:
                b.is_pinned = body.is_pinned
            b.last_accessed = datetime.now()
            store.save_blocks(blocks)
            return {"ok": True, "block": b.model_dump(mode="json")}
    raise HTTPException(status_code=404, detail="记忆块不存在")


@router.delete("/blocks/{block_id}")
def delete_block(block_id: str):
    store = get_store()
    blocks = store.load_blocks()
    rest = [b for b in blocks if b.id != block_id]
    if len(rest) == len(blocks):
        raise HTTPException(status_code=404, detail="记忆块不存在")
    store.save_blocks(rest)
    return {"ok": True, "deleted": block_id}


# ---------- 水位与真实对话 tokens ----------
@router.get("/status")
def status():
    store = get_store()
    conv = store.load_conversation()
    blocks = store.load_blocks()
    tokens = int(conv.get("tokens", 0))
    pool_tokens = sum(b.token_count for b in blocks)
    active = sum(1 for b in blocks if b.status == BlockStatus.ACTIVE)
    return {
        "ok": True,
        "conversation_tokens": tokens,
        "round": int(conv.get("round", 0)),
        "level": watermark.watermark_level(tokens),
        "needs_compact": watermark.should_compact(tokens),
        "over_limit": watermark.is_over_limit(tokens),
        "needs_handoff": handoff.needs_handoff(store),
        "pool_tokens": pool_tokens,
        "active_count": active,
        "total_count": len(blocks),
        "thresholds": {
            "compaction": watermark.COMPACTION_THRESHOLD,
            "max": watermark.MAX_LIMIT,
        },
        "urgency": watermark.compact_urgency(tokens),
    }


@router.post("/usage")
def push_usage(body: UsagePush):
    store = get_store()
    cur = store.update_conversation(body.tokens, body.round_inc)
    return {"ok": True, "conversation": cur}


# ---------- 压缩 ----------
@router.post("/compress")
def run_compress():
    return compactor.compress(get_store())


# ---------- 交接 ----------
@router.post("/handoff/preview")
def handoff_preview(body: Optional[GoalBody] = None):
    store = get_store()
    goal = (body.goal if body else "") or ""
    md = handoff.generate_markdown(store, goal)
    return {
        "ok": True,
        "markdown": md,
        "tokens": watermark.count_tokens(md),
        "chars": len(md),
        "needs": handoff.needs_handoff(store),
    }


@router.post("/handoff/confirm")
def handoff_confirm(body: Optional[GoalBody] = None):
    goal = (body.goal if body else "") or ""
    return handoff.confirm(get_store(), goal)


@router.post("/handoff/rollback")
def handoff_rollback():
    """紧急回滚（隐藏接口）：恢复 30 分钟窗口内最近一次快照。"""
    store = get_store()
    ok = store.rollback_latest()
    return {"ok": ok, "message": "已回滚最近一次记忆快照。" if ok else "没有可回滚的快照。"}


# ---------- 阶段总结（按阶段工作） ----------
@router.post("/phase/preview")
def phase_preview(body: Optional[GoalBody] = None):
    goal = (body.goal if body else "") or ""
    return phase.preview(get_store(), goal)


@router.post("/phase/confirm")
def phase_confirm(body: Optional[PhaseConfirmBody] = None):
    b = body or PhaseConfirmBody()
    return phase.confirm(
        get_store(),
        goal=(b.goal or ""),
        summary=b.summary,
        open_new_thread=bool(b.open_new_thread),
    )


@router.get("/phases")
def phases_list():
    return {"ok": True, "phases": phase.list_phases(get_store())}
