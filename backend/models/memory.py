# -*- coding: utf-8 -*-
"""记忆块数据模型（Pydantic）。

按规格文档原样实现：MemoryBlock + BlockType/BlockStatus 枚举。
"""
from pydantic import BaseModel
from typing import Optional, List
from enum import Enum
from datetime import datetime


class BlockType(str, Enum):
    FACT = "fact"
    PREFERENCE = "preference"
    TASK = "task"
    CODE_SNIPPET = "code_snippet"
    PLAN = "plan"
    CONSTRAINT = "constraint"
    USER_DEFINED = "user_defined"
    PHASE = "phase"            # 阶段总结块：用户确认阶段总结后生成（受保护）


class BlockStatus(str, Enum):
    ACTIVE = "active"
    PAUSED = "paused"          # 用户手动休眠，不进入上下文
    PROBATION = "probation"    # 刚压缩生成，观察期
    DEPRECATED = "deprecated"


class MemoryBlock(BaseModel):
    id: str                     # 格式: mem_{timestamp}_{uuid4前8位}
    type: BlockType
    content: str                # 精炼描述，建议 ≤ 50 字
    importance: int             # 1-5，5为最高（受保护）
    status: BlockStatus
    token_count: int            # 使用 tiktoken 实时计算
    last_accessed: datetime
    source_round: int           # 对话轮次索引
    deprecated_ids: Optional[List[str]] = None
    is_pinned: bool = False     # 用户置顶
    order_index: int = 0        # 拖拽排序基准
