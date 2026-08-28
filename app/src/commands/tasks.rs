//! R9 任务/产出物命令：按任务（轮次）读取文件变更（含 diff 前后内容）。

use crate::app_state::AppState;
use oh_core::codex::ThreadFileChange;
use tauri::State;

/// 读取指定会话中某一轮（任务）的文件变更，用于产出物卡片与 Diff。
#[tauri::command]
pub(crate) async fn task_artifacts(
    state: State<'_, AppState>,
    thread_id: String,
    turn_id: String,
) -> Result<Vec<ThreadFileChange>, String> {
    let mut guard = state.engine.lock().await;
    let server = guard
        .as_mut()
        .ok_or_else(|| "引擎未启动，无法读取任务产出物。".to_string())?;
    server
        .read_thread_file_changes(&thread_id, &turn_id)
        .await
        .map_err(|e| format!("读取产出物失败: {}", e))
}
