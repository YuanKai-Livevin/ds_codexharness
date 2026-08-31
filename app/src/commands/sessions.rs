//! 会话管理命令：列表/切换/新建/删除/历史 + 临时文件清理。

use crate::app_state::{data_root, AppState};
use crate::services::memory_sidecar::{memory_block_dir, reset_memory_conversation};
use oh_core::workspace;
use tauri::State;

/// 列出当前工作区下的会话（任务历史）。
#[tauri::command]
pub(crate) async fn list_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<oh_core::codex::ThreadInfo>, String> {
    let mut guard = state.engine.lock().await;
    let server = guard.as_mut().ok_or_else(|| "引擎未启动".to_string())?;
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    server
        .list_threads(&ws.to_string_lossy())
        .await
        .map_err(|e| e.to_string())
}

/// 当前会话 id。
#[tauri::command]
pub(crate) async fn current_session(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let guard = state.engine.lock().await;
    match guard.as_ref() {
        Some(server) => Ok(server.current_thread_id().await),
        None => Ok(None),
    }
}

/// 新建会话。
#[tauri::command]
pub(crate) async fn new_session(state: State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.engine.lock().await;
    let server = guard.as_mut().ok_or_else(|| "引擎未启动".to_string())?;
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let skills_repo = data_root().join("skills").to_string_lossy().to_string();
    let tid = server
        .start_thread(
            &ws.to_string_lossy(),
            &s.sandbox_mode,
            &s.model,
            &skills_repo,
        )
        .await
        .map_err(|e| format!("新建会话失败: {}", e))?;
    // 新会话 = 全新上下文：复位记忆面板水位（tokens/round 归零）
    reset_memory_conversation(&memory_block_dir(&ws).join("data")).await;
    Ok(tid)
}

/// 切换到指定会话。
#[tauri::command]
pub(crate) async fn switch_session(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), String> {
    let mut guard = state.engine.lock().await;
    let server = guard.as_mut().ok_or_else(|| "引擎未启动".to_string())?;
    server
        .resume_thread(&thread_id)
        .await
        .map_err(|e| format!("切换会话失败: {}", e))?;
    Ok(())
}

/// 删除指定会话（若删除的是当前会话，自动新建一个）。
#[tauri::command]
pub(crate) async fn delete_session(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<String, String> {
    let mut guard = state.engine.lock().await;
    let server = guard.as_mut().ok_or_else(|| "引擎未启动".to_string())?;
    let is_current = server.current_thread_id().await.as_deref() == Some(thread_id.as_str());
    server
        .delete_thread(&thread_id)
        .await
        .map_err(|e| format!("删除会话失败: {}", e))?;
    if is_current {
        let s = state.settings.lock().await.clone();
        let ws = workspace::ensure_workspace(&s.workspace_path)?;
        let skills_repo = data_root().join("skills").to_string_lossy().to_string();
        let tid = server
            .start_thread(
                &ws.to_string_lossy(),
                &s.sandbox_mode,
                &s.model,
                &skills_repo,
            )
            .await
            .map_err(|e| format!("新建会话失败: {}", e))?;
        Ok(tid)
    } else {
        Ok(String::new())
    }
}

/// 读取会话历史消息（用户/助手文本）。
#[tauri::command]
pub(crate) async fn session_history(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<Vec<oh_core::codex::HistoryMessage>, String> {
    let mut guard = state.engine.lock().await;
    let server = guard.as_mut().ok_or_else(|| "引擎未启动".to_string())?;
    server
        .read_thread_history(&thread_id)
        .await
        .map_err(|e| e.to_string())
}

/// 列出工作区 .oh_tmp 临时文件（用于清理确认）。
#[tauri::command]
pub(crate) async fn list_tmp(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let tmp = ws.join(".oh_tmp");
    if !tmp.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(&tmp)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        out.push(e.file_name().to_string_lossy().to_string());
    }
    Ok(out)
}

/// 清理 .oh_tmp 下的临时文件（破坏性操作，前端需二次确认）。
#[tauri::command]
pub(crate) async fn cleanup_tmp(state: State<'_, AppState>) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let tmp = ws.join(".oh_tmp");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    Ok(())
}
