//! 记忆面板命令：状态查询与打开面板。

use crate::app_state::AppState;
use crate::services::memory_sidecar::{memory_block_dir, session_token};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

/// 记忆面板状态（供前端显示服务可用性与 iframe 地址）。
#[derive(Serialize, Clone)]
pub(crate) struct MemoryStatus {
    running: bool,
    port: u16,
    token: String,
    workspace: String,
    data_dir: String,
    panel_url: String,
    deployed: bool,
}

#[tauri::command]
pub(crate) async fn memory_status(state: State<'_, AppState>) -> Result<MemoryStatus, String> {
    let s = state.settings.lock().await.clone();
    let ws = std::path::PathBuf::from(&s.workspace_path);
    let block = memory_block_dir(&ws);
    let data_dir = block.join("data");
    let pid = *state.memory_pid.lock().await;
    let running = pid.map(oh_core::winproc::process_alive).unwrap_or(false);
    let port = *state.memory_port.lock().await;
    let token = if running {
        session_token(&state).await?
    } else {
        String::new()
    };
    let deployed = block.join("frontend").join("sidebar.html").exists();
    let panel_url = match port {
        Some(p) if !token.is_empty() => format!("http://127.0.0.1:{}/?token={}", p, token),
        _ => block.join("frontend").join("sidebar.html").to_string_lossy().to_string(),
    };
    Ok(MemoryStatus {
        running,
        port: port.unwrap_or(0),
        token,
        workspace: s.workspace_path,
        data_dir: data_dir.to_string_lossy().to_string(),
        panel_url,
        deployed,
    })
}

/// 打开记忆面板（默认浏览器，带令牌 URL）。
#[tauri::command]
pub(crate) async fn open_memory_panel(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let st = memory_status(state).await?;
    if !st.running || st.panel_url.is_empty() {
        return Err("记忆服务未运行，无法打开面板。".to_string());
    }
    let quoted = st.panel_url.replace('\'', "''");
    let mut cmd = std::process::Command::new("powershell");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.args([
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-Command",
        &format!("Start-Process -FilePath '{}'", quoted),
    ])
    .spawn()
    .map_err(|e| e.to_string())?;
    Ok(())
}
