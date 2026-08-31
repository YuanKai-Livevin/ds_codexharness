//! 记忆面板命令：状态查询与打开面板。

use crate::app_state::AppState;
use crate::services::memory_sidecar::{memory_block_dir, session_token};
use serde::Serialize;
use tauri::{AppHandle, State};

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
        _ => block
            .join("frontend")
            .join("sidebar.html")
            .to_string_lossy()
            .to_string(),
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
pub(crate) async fn open_memory_panel(
    state: State<'_, AppState>,
    _app: AppHandle,
) -> Result<(), String> {
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

/// 记忆 API 代理（Rust → 本地记忆服务）：绕开 WebView2 iframe 加载 http 的限制，
/// 主应用直接经此命令读写记忆数据（本地回环 + 会话令牌鉴权 + 路径白名单）。
#[tauri::command]
pub(crate) async fn memory_api(
    state: State<'_, AppState>,
    path: String,
    method: String,
    body: Option<String>,
) -> Result<String, String> {
    use std::io::{Read, Write};
    // 路径白名单：只允许记忆 API 前缀，杜绝任意路径
    if !(path.starts_with("/memory/") || path.starts_with("/api/memory/")) {
        return Err("非法路径（仅允许 /memory/*）".to_string());
    }
    // 后端路由挂在 /api/memory/* 下，统一补 /api 前缀
    let req_path = if path.starts_with("/api/") {
        path.clone()
    } else {
        format!("/api{}", path)
    };
    let port = *state.memory_port.lock().await;
    let port = port.ok_or_else(|| "记忆服务未运行".to_string())?;
    let token = session_token(&state).await?;
    let method = if method.trim().is_empty() {
        "GET".to_string()
    } else {
        method.trim().to_uppercase()
    };
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "DELETE") {
        return Err(format!("不支持的方法: {}", method));
    }
    let body = body.unwrap_or_default();
    let req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        method,
        req_path,
        port,
        token,
        body.len(),
        body
    );
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("连接记忆服务失败: {}", e))?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(20)));
    s.write_all(req.as_bytes())
        .map_err(|e| format!("发送请求失败: {}", e))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(e) => return Err(format!("读取响应失败: {}", e)),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let status_ok = text.starts_with("HTTP/1.1 200") || text.starts_with("HTTP/1.0 200");
    let body_part = text.split("\r\n\r\n").nth(1).unwrap_or("");
    if !status_ok {
        return Err(format!(
            "记忆服务返回错误: {}",
            body_part.chars().take(300).collect::<String>()
        ));
    }
    Ok(body_part.to_string())
}
