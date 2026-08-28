# -*- coding: utf-8 -*-
"""本地 SQLite 存储层（R5：替代 JSON 作为主存储）。

- 记忆块 / 阶段记录 / 元数据 / 快照 → SQLite（memory.db，WAL）
- 对话水位保持 {data_dir}/conversation.json（Rust 侧每轮 turn 直接写文件，共享契约）
- 旧 memory.json / phases.json 首次启动自动迁移进 SQLite
- 每次保存同步导出 memory.json 作备份（供人工查阅/兼容）
- 快照保留 30 分钟供紧急回滚
"""
import json
import os
import sqlite3
import threading
from datetime import datetime, timedelta
from typing import List, Optional

from ..models.memory import BlockStatus, MemoryBlock

ROLLBACK_WINDOW_MINUTES = 30  # 交接撤销窗口（规格：30 分钟）


def _now_iso() -> str:
    return datetime.now().isoformat(timespec="seconds")


_BLOCK_COLS = ("id", "type", "content", "importance", "status", "token_count",
               "last_accessed", "source_round", "deprecated_ids", "is_pinned", "order_index")


class MemoryStore:
    def __init__(self, data_dir: str):
        self.data_dir = data_dir
        self.db_path = os.path.join(data_dir, "memory.db")
        os.makedirs(self.data_dir, exist_ok=True)
        self._lock = threading.RLock()
        self._init_db()
        self._migrate_json()

    # ---------- 连接 ----------
    def _conn(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.db_path, timeout=10)
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        return conn

    def _init_db(self) -> None:
        with self._lock, self._conn() as conn:
            conn.executescript(
                """
                CREATE TABLE IF NOT EXISTS blocks(
                    id TEXT PRIMARY KEY,
                    type TEXT NOT NULL,
                    content TEXT NOT NULL,
                    importance INTEGER NOT NULL DEFAULT 3,
                    status TEXT NOT NULL,
                    token_count INTEGER NOT NULL DEFAULT 0,
                    last_accessed TEXT NOT NULL,
                    source_round INTEGER NOT NULL DEFAULT 0,
                    deprecated_ids TEXT,
                    is_pinned INTEGER NOT NULL DEFAULT 0,
                    order_index INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS phases(
                    id TEXT PRIMARY KEY,
                    goal TEXT NOT NULL DEFAULT '',
                    summary TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0,
                    archived_ids TEXT,
                    open_new_thread INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS meta(
                    key TEXT PRIMARY KEY,
                    value TEXT
                );
                CREATE TABLE IF NOT EXISTS snapshots(
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    created_at TEXT NOT NULL,
                    reason TEXT NOT NULL DEFAULT 'auto',
                    payload TEXT NOT NULL
                );
                """
            )

    def _migrate_json(self) -> None:
        """旧 JSON 数据（memory.json / phases.json）首次启动自动导入 SQLite。"""
        p = self.memory_path()
        if not os.path.exists(p):
            return
        with self._lock, self._conn() as conn:
            n = conn.execute("SELECT COUNT(*) c FROM blocks").fetchone()["c"]
            if n > 0:
                return
            try:
                raw = json.load(open(p, encoding="utf-8"))
                blocks = raw.get("blocks", []) if isinstance(raw, dict) else raw
                for b in blocks:
                    self._insert_block(conn, b)
                if isinstance(raw, dict):
                    for k in ("current_round", "consecutive_ineffective", "last_compress", "last_phase"):
                        if k in raw:
                            conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                                         (k, json.dumps(raw[k], ensure_ascii=False)))
            except Exception:
                pass
        pp = self.phases_path()
        if os.path.exists(pp):
            try:
                phases = json.load(open(pp, encoding="utf-8"))
                for ph in phases:
                    self.append_phase(ph)
            except Exception:
                pass

    @staticmethod
    def _insert_block(conn: sqlite3.Connection, b: dict) -> None:
        conn.execute(
            "INSERT OR REPLACE INTO blocks VALUES(?,?,?,?,?,?,?,?,?,?,?)",
            (
                b.get("id", ""),
                b.get("type", "fact"),
                b.get("content", ""),
                int(b.get("importance", 3)),
                b.get("status", "active"),
                int(b.get("token_count", 0)),
                str(b.get("last_accessed", _now_iso())),
                int(b.get("source_round", 0)),
                json.dumps(b.get("deprecated_ids"), ensure_ascii=False) if b.get("deprecated_ids") else None,
                int(bool(b.get("is_pinned", False))),
                int(b.get("order_index", 0)),
            ),
        )

    # ---------- 路径（与旧实现保持一致，供 Rust 侧共享契约） ----------
    def memory_path(self) -> str:
        return os.path.join(self.data_dir, "memory.json")

    def conversation_path(self) -> str:
        return os.path.join(self.data_dir, "conversation.json")

    def phases_path(self) -> str:
        return os.path.join(self.data_dir, "phases.json")

    # ---------- 记忆块读写 ----------
    def load_blocks(self) -> List[MemoryBlock]:
        try:
            with self._lock, self._conn() as conn:
                rows = conn.execute("SELECT * FROM blocks").fetchall()
        except Exception:
            return []
        out = []
        for r in rows:
            d = dict(r)
            if d.get("deprecated_ids"):
                try:
                    d["deprecated_ids"] = json.loads(d["deprecated_ids"])
                except Exception:
                    d["deprecated_ids"] = None
            d["is_pinned"] = bool(d["is_pinned"])
            try:
                out.append(MemoryBlock(**d))
            except Exception:
                continue
        return out

    def save_blocks(self, blocks: List[MemoryBlock], meta: Optional[dict] = None) -> None:
        with self._lock, self._conn() as conn:
            conn.execute("DELETE FROM blocks")
            for b in blocks:
                self._insert_block(conn, b.model_dump(mode="json"))
            if meta:
                for k, v in meta.items():
                    conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                                 (k, json.dumps(v, ensure_ascii=False)))
        # 同步导出 JSON 备份（供人工查阅 / 兼容旧工具）
        self._export_json(blocks, meta)

    def _export_json(self, blocks: List[MemoryBlock], meta: Optional[dict] = None) -> None:
        try:
            payload = {
                "version": 1,
                "saved_at": _now_iso(),
                **self.load_meta(),
                "blocks": [b.model_dump(mode="json") for b in blocks],
            }
            if meta:
                payload.update(meta)
            tmp = self.memory_path() + ".tmp"
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(payload, f, ensure_ascii=False, indent=2)
            os.replace(tmp, self.memory_path())
        except Exception:
            pass

    def load_meta(self) -> dict:
        meta = {"current_round": 0, "consecutive_ineffective": 0, "last_compress": None, "last_phase": None}
        try:
            with self._lock, self._conn() as conn:
                rows = conn.execute("SELECT key,value FROM meta").fetchall()
            for r in rows:
                try:
                    meta[r["key"]] = json.loads(r["value"])
                except Exception:
                    meta[r["key"]] = r["value"]
        except Exception:
            pass
        return meta

    # ---------- 阶段总结记录 ----------
    def load_phases(self) -> list:
        try:
            with self._lock, self._conn() as conn:
                rows = conn.execute("SELECT * FROM phases ORDER BY created_at").fetchall()
        except Exception:
            return []
        out = []
        for r in rows:
            d = dict(r)
            if d.get("archived_ids"):
                try:
                    d["archived_ids"] = json.loads(d["archived_ids"])
                except Exception:
                    d["archived_ids"] = []
            out.append(d)
        return out

    def append_phase(self, entry: dict) -> None:
        with self._lock, self._conn() as conn:
            conn.execute(
                "INSERT OR REPLACE INTO phases(id,goal,summary,created_at,archived,archived_ids,open_new_thread) VALUES(?,?,?,?,?,?,?)",
                (
                    entry.get("id", ""),
                    entry.get("goal", "") or "",
                    entry.get("summary", "") or "",
                    entry.get("created_at", _now_iso()),
                    int(entry.get("archived", 0)),
                    json.dumps(entry.get("archived_ids", []), ensure_ascii=False),
                    int(bool(entry.get("open_new_thread", False))),
                ),
            )

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

    # ---------- 快照与回滚（SQLite） ----------
    def snapshot(self, reason: str = "auto") -> Optional[str]:
        """快照当前记忆池与元数据，返回快照 id；无数据则返回 None。"""
        blocks = self.load_blocks()
        if not blocks:
            return None
        payload = json.dumps(
            {"blocks": [b.model_dump(mode="json") for b in blocks], "meta": self.load_meta()},
            ensure_ascii=False,
        )
        with self._lock, self._conn() as conn:
            cur = conn.execute(
                "INSERT INTO snapshots(created_at,reason,payload) VALUES(?,?,?)",
                (_now_iso(), reason, payload),
            )
            sid = cur.lastrowid
            self._prune_snapshots(conn)
        return str(sid)

    def _prune_snapshots(self, conn: sqlite3.Connection) -> None:
        cutoff = (datetime.now() - timedelta(minutes=ROLLBACK_WINDOW_MINUTES)).isoformat(timespec="seconds")
        conn.execute("DELETE FROM snapshots WHERE created_at < ?", (cutoff,))

    def list_snapshots(self) -> List[str]:
        try:
            with self._lock, self._conn() as conn:
                rows = conn.execute("SELECT id FROM snapshots ORDER BY id DESC").fetchall()
            return [str(r["id"]) for r in rows]
        except Exception:
            return []

    def rollback_latest(self) -> bool:
        """紧急回滚：恢复最近一次快照（30 分钟窗口内）。"""
        try:
            with self._lock, self._conn() as conn:
                row = conn.execute(
                    "SELECT id,payload FROM snapshots ORDER BY id DESC LIMIT 1"
                ).fetchone()
                if row is None:
                    return False
                payload = json.loads(row["payload"])
                blocks = payload.get("blocks", [])
                meta = payload.get("meta", {})
                conn.execute("DELETE FROM blocks")
                for b in blocks:
                    self._insert_block(conn, b)
                for k, v in meta.items():
                    conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                                 (k, json.dumps(v, ensure_ascii=False)))
            self._export_json(self.load_blocks(), None)
            return True
        except Exception:
            return False

    # ---------- 对话水位（真实上下文 tokens，由应用侧写文件） ----------
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
