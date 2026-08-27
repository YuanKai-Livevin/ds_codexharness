# -*- coding: utf-8 -*-
"""基于 tiktoken 的精确水位监控器。

- 使用 tiktoken.get_encoding("cl100k_base") 精确计数（中文按字节片分词）。
- 硬编码常量：COMPACTION_THRESHOLD = 52000（警戒水位）、MAX_LIMIT = 60000（红线）。
- 铁律：触发压缩的判断逻辑绝不由 LLM 决定，全部由本模块纯函数返回布尔值。

离线支持：若环境变量 TIKTOKEN_CACHE_DIR 指向包含 cl100k_base.tiktoken 的
目录（随功能块分发），tiktoken 将直接读取缓存，无需联网下载。
"""
import os
import re
from typing import List, Optional

import tiktoken

# ---- 硬编码水位常量（规格强制） ----
COMPACTION_THRESHOLD = 52000  # 警戒水位：达到后建议压缩
MAX_LIMIT = 60000             # 红线：超过后必须压缩/交接

# ---- 单例编码器（进程内复用） ----
_ENC = None

# cl100k_base 官方定义（与 tiktoken.get_encoding("cl100k_base") 完全一致）
_CL100K_PAT_STR = r"""'(?i:[sdmt]|ll|ve|re)|[^\r\n\p{L}\p{N}]?+\p{L}++|\p{N}{1,3}+| ?[^\s\p{L}\p{N}]++[\r\n]*+|\s*[\r\n]|\s+(?!\S)|\s+"""
_CL100K_SPECIAL_TOKENS = {
    "<|endoftext|>": 100257,
    "<|fim_prefix|>": 100258,
    "<|fim_middle|>": 100259,
    "<|fim_suffix|>": 100260,
    "<|endofprompt|>": 100276,
}


def _get_encoder():
    global _ENC
    if _ENC is None:
        # 优先走本地缓存目录（TIKTOKEN_CACHE_DIR），避免首次联网下载
        cache = os.environ.get("TIKTOKEN_CACHE_DIR")
        if cache and os.path.isdir(cache):
            local = os.path.join(cache, "cl100k_base.tiktoken")
            if os.path.exists(local):
                try:
                    ranks = tiktoken.load_tiktoken_bpe(local)
                    enc = tiktoken.Encoding(
                        name="cl100k_base",
                        pat_str=_CL100K_PAT_STR,
                        mergeable_ranks=ranks,
                        special_tokens=_CL100K_SPECIAL_TOKENS,
                    )
                    _ENC = enc
                    return enc
                except Exception:
                    pass
        try:
            _ENC = tiktoken.get_encoding("cl100k_base")
        except Exception:
            # 极端兜底：使用近似估算（每 1 个中文字符 ≈ 1.7 token，ASCII ≈ 0.3）
            _ENC = None
    return _ENC


def count_tokens(text: str) -> int:
    """精确统计 token 数。"""
    if not text:
        return 0
    enc = _get_encoder()
    if enc is not None:
        try:
            return len(enc.encode(text))
        except Exception:
            pass
    # 兜底估算
    cn = len(re.findall(r"[\u4e00-\u9fff]", text))
    other = len(text) - cn
    return int(cn * 1.7 + other * 0.3) + 1


def count_many(texts: List[str]) -> int:
    return sum(count_tokens(t) for t in texts)


# ---- 纯函数水位判定（铁律：绝不交给 LLM） ----

def watermark_level(tokens: int) -> str:
    """返回水位档位：'ok' | 'warn' | 'critical'。"""
    if tokens >= MAX_LIMIT:
        return "critical"
    if tokens >= COMPACTION_THRESHOLD:
        return "warn"
    return "ok"


def should_compact(tokens: int) -> bool:
    """是否应触发压缩：达到警戒水位即返回 True。"""
    return tokens >= COMPACTION_THRESHOLD


def is_over_limit(tokens: int) -> bool:
    """是否超过红线。"""
    return tokens >= MAX_LIMIT


def compact_urgency(tokens: int) -> Optional[str]:
    """返回给用户看的提示文案（None 表示无需处理）。"""
    if tokens >= MAX_LIMIT:
        return f"上下文已达红线（{tokens}/{MAX_LIMIT}），建议立即压缩并生成交接。"
    if tokens >= COMPACTION_THRESHOLD:
        return f"上下文进入警戒区（{tokens}/{COMPACTION_THRESHOLD}），建议压缩记忆以保护体验。"
    return None
