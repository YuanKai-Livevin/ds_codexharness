# -*- coding: utf-8 -*-
"""本地 JSON 存储层（替代 Redis 的离线方案）。

- 记忆块存于 {data_dir}/memory.json
- 对话水位存于 {data_dir}/conversation.json（Rust 侧每轮 turn 写入）
- 交接/压缩前自动快照到 {data_dir}/backups/，保留 30 分钟供紧急回滚
"""
import json
import os
import shutil
import threading
from datetime import datetime, timedelta
from typing import List, Optional

from ..models.memory import BlockStatus, MemoryBlock

ROLLBACK_WINDOW_MINUTES = 30  # 交接撤销窗口（规格：30 分钟）


def _now_iso() -> str:
    return datetime.now().isoformat(timespec="seconds")


class MemoryStore:
    def __init__(self, data_dir: str):
        self.data_dir = data_dir
        self.backup_dir = os.path.join(data_dir, "backups")
        os.makedirs(self.data_dir, exist_ok=True)
        os.makedirs(self.backup_dir, exist_ok=True)
        self._lock = threading.RLock()

    # ---------- 路径 ----------
    def memory_path(self) -> str:
        return os.path.join(self.data_dir, "memory.json")

    def conversation_path(self) -> str:
        return os.path.join(self.data_dir, "conversation.json")

    # ---------- 记忆块读写 ----------
    def load_blocks(self) -> List[MemoryBlock]:
        p = self.memory_path()
        if not os.path.exists(p):
            return []
        try:
            with open(p, "r", encoding="utf-8") as f:
                raw = json.load(f)
            blocks = raw.get("blocks", []) if isinstance(raw, dict) else raw
            return [MemoryBlock(**b) for b in blocks]
        except Exception:
            return []

    # ---------- 阶段总结记录 ----------
    def phases_path(self) -> str:
        return os.path.join(self.data_dir, "phases.json")

    def load_phases(self) -> list:
        p = self.phases_path()
        if not os.path.exists(p):
            return []
        try:
            with open(p, "r", encoding="utf-8") as f:
                raw = json.load(f)
            return raw if isinstance(raw, list) else []
        except Exception:
            return []

    def append_phase(self, entry: dict) -> None:
        with self._lock:
            phases = self.load_phases()
            phases.append(entry)
            tmp = self.phases_path() + ".tmp"
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(phases, f, ensure_ascii=False, indent=2)
            os.replace(tmp, self.phases_path())

    def archive_active_blocks(self, keep_pinned: bool = True) -> List[MemoryBlock]:
        """把当前阶段的所有活动记忆块归档（ACTIVE/PROBATION → DEPRECATED）。

        返回被归档的块列表。置顶块（is_pinned）保留。
        """
        blocks = self.load_blocks()
        archived = []
        rest = []
        for b in blocks:
            if b.status in (BlockStatus.ACTIVE, BlockStatus.PROBATION) and not (keep_pinned and b.is_pinned):
                b.status = BlockStatus.DEPRECATED
                archived.append(b)
            rest.append(b)
        if archived:
            self.save_blocks(rest)
        return archived

    def load_meta(self) -> dict:
        p = self.memory_path()
        meta = {"current_round": 0, "consecutive_ineffective": 0, "last_compress": None}
        if os.path.exists(p):
            try:
                with open(p, "r", encoding="utf-8") as f:
                    raw = json.load(f)
                if isinstance(raw, dict):
                    for k in meta:
                        if k in raw:
                            meta[k] = raw[k]
            except Exception:
                pass
        return meta

    def save_blocks(self, blocks: List[MemoryBlock], meta: Optional[dict] = None) -> None:
        with self._lock:
            cur = self.load_meta()
            if meta:
                cur.update(meta)
            payload = {
                "version": 1,
                "saved_at": _now_iso(),
                **cur,
                "blocks": [b.model_dump(mode="json") for b in blocks],
            }
            tmp = self.memory_path() + ".tmp"
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(payload, f, ensure_ascii=False, indent=2)
            os.replace(tmp, self.memory_path())

    def snapshot(self, reason: str = "auto") -> Optional[str]:
        """快照当前记忆池，返回快照文件路径；无数据则返回 None。"""
        p = self.memory_path()
        if not os.path.exists(p):
            return None
        name = "snap_{}_{}.json".format(datetime.now().strftime("%Y%m%d_%H%M%S"), reason)
        dst = os.path.join(self.backup_dir, name)
        shutil.copy2(p, dst)
        self._prune_backups()
        return dst

    def _prune_backups(self) -> None:
        """删除超过回滚窗口的旧快照。"""
        cutoff = datetime.now() - timedelta(minutes=ROLLBACK_WINDOW_MINUTES)
        for f in os.listdir(self.backup_dir):
            if not f.startswith("snap_"):
                continue
            fp = os.path.join(self.backup_dir, f)
            try:
                if datetime.fromtimestamp(os.path.getmtime(fp)) < cutoff:
                    os.remove(fp)
            except Exception:
                pass

    def list_snapshots(self) -> List[str]:
        out = []
        for f in sorted(os.listdir(self.backup_dir), reverse=True):
            if f.startswith("snap_"):
                out.append(os.path.join(self.backup_dir, f))
        return out

    def rollback_latest(self) -> bool:
        """紧急回滚：恢复最近一次快照（30 分钟窗口内）。"""
        snaps = self.list_snapshots()
        if not snaps:
            return False
        src = snaps[0]
        with self._lock:
            shutil.copy2(src, self.memory_path())
        return True

    # ---------- 对话水位（真实上下文 tokens，由应用侧写入） ----------
    def load_conversation(self) -> dict:
        p = self.conversation_path()
        d = {"tokens": 0, "round": 0, "updated_at": None}
        if os.path.exists(p):
            try:
                with open(p, "r", encoding="utf-8") as f:
                    d.update(json.load(f))
            except Exception:
                pass
        return d

    def update_conversation(self, tokens: int, round_inc: int = 0) -> dict:
        """更新对话水位。round_inc=1 表示新完成一轮 turn。"""
        with self._lock:
            cur = self.load_conversation()
            cur["tokens"] = max(0, int(tokens))
            cur["round"] = int(cur.get("round", 0)) + int(round_inc)
            cur["updated_at"] = _now_iso()
            tmp = self.conversation_path() + ".tmp"
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(cur, f, ensure_ascii=False, indent=2)
            os.replace(tmp, self.conversation_path())
            # 同步 meta.current_round
            blocks = self.load_blocks()
            self.save_blocks(blocks, {"current_round": cur["round"]})
            return cur
