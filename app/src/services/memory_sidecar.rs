//! 记忆面板 sidecar 服务：部署、拉起、停止与水位写入。

use crate::app_state::{AppState, MEMORY_PORT};
use oh_core::python::Bundled;
use std::path::Path;
use std::path::PathBuf;
use tauri::{AppHandle, State};

/// 记忆功能块在工作区内的目录：{工作区}/.harness-memory/
pub(crate) fn memory_block_dir(ws: &Path) -> PathBuf {
    ws.join(".harness-memory")
}

/// 把随程序分发的记忆面板代码（backend/ frontend/）部署进当前工作区。
/// 源：exe 旁 memory-block/{backend,frontend}（生产）或开发根 backend/ frontend/。
pub(crate) fn ensure_memory_block(bundled: &Bundled, ws: &Path) -> Result<PathBuf, String> {
    let root = &bundled.root;
    let src_backend = {
        let p = root.join("memory-block").join("backend");
        if p.exists() {
            p
        } else {
            root.join("backend")
        }
    };
    let src_frontend = {
        let p = root.join("memory-block").join("frontend");
        if p.exists() {
            p
        } else {
            root.join("frontend")
        }
    };
    if !src_backend.join("api").join("main.py").exists() || !src_frontend.join("sidebar.html").exists() {
        return Err("未找到记忆面板代码（memory-block/backend 与 frontend），请检查程序目录是否完整。".to_string());
    }
    let block = memory_block_dir(ws);
    let dst_backend = block.join("backend");
    let dst_frontend = block.join("frontend");
    std::fs::create_dir_all(block.join("data")).map_err(|e| e.to_string())?;
    // 覆盖式同步代码（跳过缓存与数据），保证每次更新生效
    copy_memory_dir(&src_backend, &dst_backend).map_err(|e| e.to_string())?;
    copy_memory_dir(&src_frontend, &dst_frontend).map_err(|e| e.to_string())?;
    Ok(block)
}

/// 复制记忆面板代码：跳过 __pycache__ 与 data，避免覆盖用户数据。
fn copy_memory_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            if name == "__pycache__" || name == "data" {
                continue;
            }
            copy_memory_dir(&from, &to)?;
        } else if !name.to_string_lossy().ends_with(".pyc") {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 拉起（或重启）记忆面板后端：python -m uvicorn backend.api.main:app --port 8765
pub(crate) async fn spawn_memory_server(
    _app: &AppHandle,
    state: &State<'_, AppState>,
    bundled: &Bundled,
    ws: &Path,
    key: &str,
) -> Result<u32, String> {
    stop_memory_server(state).await;
    if !bundled.python_available() {
        return Err("未找到内置 Python 运行时。".to_string());
    }
    let block = ensure_memory_block(bundled, ws)?;
    let data_dir = block.join("data");
    let py = bundled.python_exe();
    let settings = state.settings.lock().await.clone();
    let base_url = settings.base_url.clone();
    let model = settings.model.clone();
    let use_bridge = settings.use_bridge;
    let mut cmd = std::process::Command::new(&py);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW，避免黑窗
    }
    cmd.args([
        "-m", "uvicorn",
        "backend.api.main:app",
        "--host", "127.0.0.1",
        "--port", &MEMORY_PORT.to_string(),
    ])
    .current_dir(&block)
    .env("HARNESS_DATA_DIR", &data_dir)
    .env("HARNESS_WORKSPACE", ws)
    .env("TIKTOKEN_CACHE_DIR", block.join("backend").join("services").join("encodings"))
    .env("HARNESS_BASE_URL", &base_url)
    .env("HARNESS_MODEL", &model);
    // 内置翻译层：引擎指向本地 /responses，翻译到真实上游 /chat/completions
    if use_bridge {
        cmd.env("HARNESS_BRIDGE", "1")
            .env("HARNESS_UPSTREAM_URL", &base_url)
            .env("HARNESS_UPSTREAM_MODEL", &model);
    }
    if !key.is_empty() {
        cmd.env("OH_API_KEY", key);
    }
    let log_path = data_dir.join("memory-server.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("无法打开记忆服务日志 {}: {}", log_path.display(), e))?;
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log));
    let child = cmd.spawn().map_err(|e| format!("启动记忆面板服务失败: {}", e))?;
    let pid = child.id();
    *state.memory_pid.lock().await = Some(pid);
    Ok(pid)
}

/// 停止记忆面板后端（进程树）。
pub(crate) async fn stop_memory_server(state: &State<'_, AppState>) {
    let pid = *state.memory_pid.lock().await;
    if let Some(p) = pid {
        oh_core::winproc::kill_tree(p);
    }
    *state.memory_pid.lock().await = None;
}

/// 停止记忆面板后端（退出钩子用，非 async 上下文）。
pub(crate) fn stop_memory_server_sync(pid: Option<u32>) {
    if let Some(p) = pid {
        oh_core::winproc::kill_tree(p);
    }
}

/// 从 codex turn/completed 的 usage JSON 中提取上下文 tokens（输入 tokens）。
pub(crate) fn parse_input_tokens(usage: &str) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_str(usage).ok()?;
    v.get("input_tokens")
        .and_then(|x| x.as_u64())
        .or_else(|| v.pointer("/total_token_usage/input_tokens").and_then(|x| x.as_u64()))
}

/// 把 codex 每轮的真实上下文 tokens 写入记忆面板数据文件（水位监控数据源）。
pub(crate) async fn write_conversation_tokens(data_dir: &Path, tokens: u64) {
    let path = data_dir.join("conversation.json");
    let prev_round = match tokio::fs::read_to_string(&path).await {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) => v.get("round").and_then(|x| x.as_u64()).unwrap_or(0),
            Err(_) => 0,
        },
        Err(_) => 0,
    };
    let payload = serde_json::json!({
        "tokens": tokens,
        "round": prev_round + 1,
        "updated_at": chrono_like_now(),
    });
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let tmp = data_dir.join("conversation.json.tmp");
    if tokio::fs::write(&tmp, serde_json::to_string(&payload).unwrap_or_default()).await.is_ok() {
        let _ = tokio::fs::rename(&tmp, &path).await;
    }
}

/// 复位记忆面板的对话水位文件（新会话/新阶段时调用）。
pub(crate) async fn reset_memory_conversation(data_dir: &Path) {
    let payload = serde_json::json!({
        "tokens": 0,
        "round": 0,
        "updated_at": chrono_like_now(),
    });
    let path = data_dir.join("conversation.json");
    let _ = tokio::fs::create_dir_all(data_dir).await;
    let tmp = data_dir.join("conversation.json.tmp");
    if tokio::fs::write(&tmp, serde_json::to_string(&payload).unwrap_or_default()).await.is_ok() {
        let _ = tokio::fs::rename(&tmp, &path).await;
    }
}

/// 轻量 ISO8601（无 chrono 依赖）：Windows 下用 SystemTime。
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    let rem = secs % 86400;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// 天数 → (年,月,日)（civil_from_days 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
