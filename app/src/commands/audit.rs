//! R6 审计命令：查询审计记录、标记任务接受、导出诊断包（用户主动触发）。

use crate::app_state::AppState;
use crate::services::audit::AuditRow;
use tauri::{AppHandle, Manager, State};

/// 查询最近 N 条审计记录（时间倒序）。
#[tauri::command]
pub(crate) async fn audit_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<AuditRow>, String> {
    state.audit.list(limit.unwrap_or(300))
}

/// 标记任务最终是否被用户接受（task_end 行）。
#[tauri::command]
pub(crate) async fn audit_accept(
    state: State<'_, AppState>,
    task_id: String,
    accepted: bool,
) -> Result<(), String> {
    state.audit.mark_accepted(&task_id, accepted)
}

/// 导出诊断包（JSON）：审计记录 + 引擎日志尾部 + 脱敏后的设置。
/// 只由用户主动调用；不包含 API Key 明文/密文。
#[tauri::command]
pub(crate) async fn audit_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let rows = state.audit.all()?;

    // 脱敏后的设置：只挑安全字段，绝不包含 api_key_enc
    let s = state.settings.lock().await.clone();
    let settings_redacted = serde_json::json!({
        "workspace_path": s.workspace_path,
        "provider_name": s.provider_name,
        "base_url": s.base_url,
        "model": s.model,
        "sandbox_mode": s.sandbox_mode,
        "windows_sandbox": s.windows_sandbox,
        "log_dir": s.log_dir,
        "no_auth": s.no_auth,
        "use_bridge": s.use_bridge,
    });

    // 引擎日志尾部（最近 200 行，脱敏）
    let log_path = std::path::Path::new(&s.log_dir).join("harness.log");
    let log_tail = std::fs::read_to_string(&log_path)
        .map(|t| {
            let lines: Vec<&str> = t.lines().collect();
            let tail: Vec<&str> = lines.iter().rev().take(200).copied().collect();
            let joined = tail.iter().rev().cloned().collect::<Vec<_>>().join("\n");
            crate::services::audit::redact(&joined)
        })
        .unwrap_or_else(|_| "(无日志文件)".to_string());

    let ts = chrono_like_now();
    let package = serde_json::json!({
        "app": "JONHON Harness",
        "version": "0.3",
        "exported_at": ts,
        "note": "诊断包包含审计记录与引擎日志（已脱敏），不含 API Key。仅用于排查问题。",
        "settings_redacted": settings_redacted,
        "audit_rows": rows,
        "engine_log_tail": log_tail,
    });

    let handle = tauri::async_runtime::spawn(async move {
        rfd::AsyncFileDialog::new()
            .set_title("导出诊断包（已脱敏，不含 API Key）")
            .add_filter("JSON", &["json"])
            .set_file_name(format!("harness-diagnostics-{}.json", ts.replace([':', ' '], "-")))
            .save_file()
            .await
    });
    let file = handle.await.map_err(|e| format!("保存对话框异常: {}", e))?;
    let path = match file {
        Some(f) => f.path().to_path_buf(),
        None => return Err("已取消导出".to_string()),
    };
    let text = serde_json::to_string_pretty(&package).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("写入诊断包失败: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

/// 轻量 ISO 时间戳（无 chrono 依赖）。
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Windows 上无 libc localtime 可用，直接按 UTC+8 粗略显示（仅供文件名/展示）
    let d = secs + 8 * 3600;
    let days = d / 86400;
    let rem = d % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // 1970-01-01 起的天数 → 简单推算年/月/日
    let (y, mo, da) = civil_from_days(days as i64);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, da, h, m, s)
}

/// 天数 → 公历日期（Howard Hinnant 算法，无依赖）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// 供 engine.rs 等模块复用的时间戳（与审计 ts 一致）。
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
