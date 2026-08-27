//! 记忆面板命令：状态查询与打开面板。

use crate::app_state::{AppState, MEMORY_PORT};
use crate::services::memory_sidecar::{ensure_memory_block, memory_block_dir};
use oh_core::python::Bundled;
use oh_core::workspace;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

/// 记忆面板状态（供前端显示服务可用性）。
#[derive(Serialize, Clone)]
pub(crate) struct MemoryStatus {
    running: bool,
    port: u16,
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
    let deployed = block.join("frontend").join("sidebar.html").exists();
    Ok(MemoryStatus {
        running,
        port: MEMORY_PORT,
        workspace: s.workspace_path,
        data_dir: data_dir.to_string_lossy().to_string(),
        panel_url: block.join("frontend").join("sidebar.html").to_string_lossy().to_string(),
        deployed,
    })
}

/// 打开记忆面板（默认浏览器）。
#[tauri::command]
pub(crate) async fn open_memory_panel(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let block = ensure_memory_block(&bundled, &ws)?;
    let panel = block.join("frontend").join("sidebar.html");
    if !panel.exists() {
        return Err("记忆面板文件不存在。".to_string());
    }
    let quoted = panel.to_string_lossy().replace('\'', "''");
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
