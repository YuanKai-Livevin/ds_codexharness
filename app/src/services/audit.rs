//! R6 结构化审计与诊断（T1-07）。
//!
//! 独立 SQLite 库 audit.db（C:\HARNESS\audit），追加式记录：
//! - 任务（回合）：用户目标、模型/网关、工作区、耗时、token、成本、是否接受；
//! - 工具调用（命令+状态+输出大小，内容不进库）；
//! - 文件变更摘要；
//! - 审批请求与决策；
//! - 错误与重试、引擎生命周期。
//!
//! 写入前统一脱敏（redact）：含密钥/口令关键词的行被掩码，不落盘明文。

use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

/// 审计行（返回前端 / 导出诊断包）。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AuditRow {
    pub id: i64,
    pub ts: i64,
    pub task_id: Option<String>,
    pub category: String,
    pub event: String,
    pub detail: String,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub duration_ms: Option<i64>,
    pub cost: Option<f64>,
    pub accepted: Option<i64>,
}

/// 审计存储：单连接 + 互斥锁（写量小，同步即可；不跨 await 持锁）。
pub(crate) struct AuditStore {
    conn: Mutex<rusqlite::Connection>,
}

impl AuditStore {
    /// 打开（或创建）审计库。
    pub(crate) fn open(root: &Path) -> Self {
        let dir = root.join("audit");
        let _ = std::fs::create_dir_all(&dir);
        let conn = rusqlite::Connection::open(dir.join("audit.db")).unwrap_or_else(|e| {
            // 打不开就退化为内存库（不影响主功能）
            eprintln!("audit: open failed: {e}");
            rusqlite::Connection::open_in_memory().expect("in-memory audit db")
        });
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                task_id TEXT,
                category TEXT NOT NULL,
                event TEXT NOT NULL,
                detail TEXT,
                tokens_in INTEGER,
                tokens_out INTEGER,
                duration_ms INTEGER,
                cost REAL,
                accepted INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_audit_task ON audit_events(task_id);
            CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_events(ts);
            CREATE INDEX IF NOT EXISTS idx_audit_cat ON audit_events(category);",
        );
        Self { conn: Mutex::new(conn) }
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// 记录一条审计事件。detail 会先经 JSON 序列化再整体脱敏。
    pub(crate) fn record(
        &self,
        task_id: Option<&str>,
        category: &str,
        event: &str,
        detail: serde_json::Value,
    ) {
        self.record_full(
            task_id,
            category,
            event,
            detail,
            None,
            None,
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_full(
        &self,
        task_id: Option<&str>,
        category: &str,
        event: &str,
        detail: serde_json::Value,
        tokens_in: Option<i64>,
        tokens_out: Option<i64>,
        duration_ms: Option<i64>,
        cost: Option<f64>,
        accepted: Option<i64>,
    ) {
        let detail_str = redact(&detail.to_string());
        let g = match self.conn.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let _ = g.execute(
            "INSERT INTO audit_events
                (ts, task_id, category, event, detail, tokens_in, tokens_out, duration_ms, cost, accepted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                Self::now_ms(),
                task_id,
                category,
                event,
                detail_str,
                tokens_in,
                tokens_out,
                duration_ms,
                cost,
                accepted
            ],
        );
    }

    /// 标记任务最终是否被用户接受。
    pub(crate) fn mark_accepted(&self, task_id: &str, accepted: bool) -> Result<(), String> {
        let g = self
            .conn
            .lock()
            .map_err(|_| "审计库锁不可用".to_string())?;
        g.execute(
            "UPDATE audit_events SET accepted = ?1 WHERE task_id = ?2 AND event = 'task_end'",
            rusqlite::params![if accepted { 1 } else { 0 }, task_id],
        )
        .map_err(|e| format!("更新审计记录失败: {}", e))?;
        Ok(())
    }

    /// 按时间倒序取最近 N 条。
    pub(crate) fn list(&self, limit: usize) -> Result<Vec<AuditRow>, String> {
        let g = self
            .conn
            .lock()
            .map_err(|_| "审计库锁不可用".to_string())?;
        let limit = limit.clamp(1, 2000) as i64;
        let mut stmt = g
            .prepare(
                "SELECT id, ts, task_id, category, event, detail,
                        tokens_in, tokens_out, duration_ms, cost, accepted
                 FROM audit_events ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("查询审计失败: {}", e))?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |r| {
                Ok(AuditRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    task_id: r.get(2)?,
                    category: r.get(3)?,
                    event: r.get(4)?,
                    detail: r.get(5)?,
                    tokens_in: r.get(6)?,
                    tokens_out: r.get(7)?,
                    duration_ms: r.get(8)?,
                    cost: r.get(9)?,
                    accepted: r.get(10)?,
                })
            })
            .map_err(|e| format!("读取审计失败: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取审计失败: {}", e))?;
        Ok(rows)
    }

    /// 全部行（诊断包导出用）。
    pub(crate) fn all(&self) -> Result<Vec<AuditRow>, String> {
        self.list(2000)
    }
}

/// 从 codex turn/completed 的 usage JSON 提取输入/输出 tokens。
pub(crate) fn parse_usage_tokens(usage: &str) -> (Option<i64>, Option<i64>) {
    let v: serde_json::Value = match serde_json::from_str(usage) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let inp = v
        .get("input_tokens")
        .and_then(|x| x.as_i64())
        .or_else(|| v.pointer("/total_token_usage/input_tokens").and_then(|x| x.as_i64()));
    let outp = v
        .get("output_tokens")
        .and_then(|x| x.as_i64())
        .or_else(|| v.pointer("/total_token_usage/output_tokens").and_then(|x| x.as_i64()));
    (inp, outp)
}

/// 已知公开模型的价格（美元 / 百万 token）；未知模型返回 None（界面显示 —）。
pub(crate) fn estimate_cost(model: &str, tin: Option<i64>, tout: Option<i64>) -> Option<f64> {
    let (pi, po) = match model {
        "deepseek-chat" => (0.27_f64, 1.10_f64),
        "deepseek-reasoner" => (0.55_f64, 2.19_f64),
        _ => return None,
    };
    let tin = tin? as f64;
    let tout = tout? as f64;
    Some(tin / 1_000_000.0 * pi + tout / 1_000_000.0 * po)
}

/// 简单脱敏：含密钥/口令类关键词的行掩码为「前缀…[已脱敏]」。
/// 无正则依赖；行级掩码对命令与详情文本足够保守。
pub(crate) fn redact(s: &str) -> String {
    let mut out = String::new();
    for line in s.split('\n') {
        let lower = line.to_ascii_lowercase();
        let sensitive = lower.contains("sk-")
            || lower.contains("api_key")
            || lower.contains("apikey")
            || lower.contains("password")
            || lower.contains("passwd")
            || lower.contains("secret")
            || lower.contains("authorization")
            || lower.contains("bearer ")
            || lower.contains("access_token")
            || lower.contains("refresh_token");
        if sensitive {
            let head: String = line.chars().take(16).collect();
            out.push_str(&head);
            out.push_str("…[已脱敏]\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_masks_secrets() {
        let clean = redact("统计文件数量");
        assert_eq!(clean, "统计文件数量");

        let cmd = "curl -H \"Authorization: Bearer sk-abc123XYZ456\" https://x";
        let r = redact(cmd);
        assert!(r.contains("[已脱敏]"));
        assert!(!r.contains("sk-abc123XYZ456"));

        let key = "export OPENAI_API_KEY=sk-proj-zzzz9999";
        let r = redact(key);
        assert!(r.contains("[已脱敏]"));
        assert!(!r.contains("sk-proj-zzzz9999"));
    }

    #[test]
    fn usage_and_cost() {
        let (i, o) = parse_usage_tokens(
            r#"{"input_tokens": 100, "output_tokens": 50, "total_token_usage": {"input_tokens": 100, "output_tokens": 50}}"#,
        );
        assert_eq!(i, Some(100));
        assert_eq!(o, Some(50));
        // 未知模型无估价
        assert!(estimate_cost("deepseek-v4-flash", Some(1_000_000), Some(100_000)).is_none());
        // 已知模型有估价
        let c = estimate_cost("deepseek-chat", Some(1_000_000), Some(1_000_000)).unwrap();
        assert!(c > 0.0);
    }

    #[test]
    fn store_roundtrip_and_accepted() {
        let dir = std::env::temp_dir().join(format!("audit-test-{}", std::process::id()));
        let store = AuditStore::open(&dir);
        store.record_full(
            Some("task-1"),
            "task",
            "task_end",
            serde_json::json!({"status": "completed"}),
            Some(100),
            Some(50),
            Some(1234),
            Some(0.001),
            None,
        );
        store.record(Some("task-1"), "tool", "command_completed", serde_json::json!({"command": "Get-ChildItem"}));
        let rows = store.list(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].category, "tool");
        assert_eq!(rows[1].category, "task");
        store.mark_accepted("task-1", true).unwrap();
        let rows = store.list(10).unwrap();
        assert_eq!(rows[1].accepted, Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
