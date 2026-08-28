//! 本地 sidecar 服务（R2）：模型网关 + 记忆服务。
//! - 独立进程、随机端口、每次启动生成会话令牌（BCryptGenRandom）
//! - 拉起后带令牌做 health 握手，未就绪视为启动失败（fail closed）
//! - 网关随引擎生命周期启停；记忆服务随应用生命周期

use crate::app_state::AppState;
use oh_core::dpapi;
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

/// 取一个空闲随机端口（绑定 0 后释放）。
fn pick_free_port() -> Result<u16, String> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("获取空闲端口失败: {}", e))?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    drop(l);
    Ok(port)
}

/// 会话令牌：首次生成（加密级随机数），此后复用。
pub(crate) async fn session_token(state: &State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.session_token.lock().await;
    if let Some(t) = guard.as_ref() {
        return Ok(t.clone());
    }
    let t = dpapi::random_hex(24).ok_or_else(|| "生成会话令牌失败".to_string())?;
    *guard = Some(t.clone());
    Ok(t)
}

/// 用原始 TCP 发最小 HTTP GET 探测 /api/health（带令牌）。
fn probe_health(port: u16, token: &str) -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let req = format!(
            "GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            port, token
        );
        if s.write_all(req.as_bytes()).is_ok() {
            let mut buf = [0u8; 512];
            if let Ok(n) = s.read(&mut buf) {
                let resp = String::from_utf8_lossy(&buf[..n]);
                return resp.starts_with("HTTP/1.1 200") || resp.starts_with("HTTP/1.0 200");
            }
        }
    }
    false
}

/// 带令牌的 health 握手（最多 10 秒）。
async fn wait_ready(port: u16, token: &str) -> bool {
    for _ in 0..20 {
        if probe_health(port, token) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

fn spawn_uvicorn(
    bundled: &Bundled,
    block: &Path,
    port: u16,
    module: &str,
    envs: &[(&str, &str)],
    log_path: &Path,
) -> Result<u32, String> {
    let py = bundled.python_exe();
    let mut cmd = std::process::Command::new(&py);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.args([
        "-m", "uvicorn", module,
        "--host", "127.0.0.1",
        "--port", &port.to_string(),
    ])
    .current_dir(block);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("无法打开日志 {}: {}", log_path.display(), e))?;
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log));
    let child = cmd.spawn().map_err(|e| format!("启动 sidecar 失败: {}", e))?;
    Ok(child.id())
}

/// 拉起模型网关（/responses 翻译，随引擎生命周期）。
pub(crate) async fn spawn_gateway(
    state: &State<'_, AppState>,
    bundled: &Bundled,
    ws: &Path,
    key: &str,
) -> Result<u16, String> {
    stop_gateway(state).await;
    let block = ensure_memory_block(bundled, ws)?;
    if !bundled.python_available() {
        return Err("未找到内置 Python 运行时。".to_string());
    }
    let token = session_token(state).await?;
    let port = pick_free_port()?;
    let settings = state.settings.lock().await.clone();
    let base_url = settings.base_url.clone();
    let model = settings.model.clone();
    let log_path = block.join("data").join("gateway.log");
    let pid = spawn_uvicorn(
        bundled,
        &block,
        port,
        "backend.gateway.main:app",
        &[
            ("HARNESS_BRIDGE", "1"),
            ("HARNESS_TOKEN", &token),
            ("HARNESS_UPSTREAM_URL", &base_url),
            ("HARNESS_UPSTREAM_MODEL", &model),
            ("HARNESS_BASE_URL", &base_url),
            ("HARNESS_MODEL", &model),
            ("OH_API_KEY", key),
        ],
        &log_path,
    )?;
    if !wait_ready(port, &token).await {
        let _ = stop_gateway(state).await;
        return Err(format!("模型网关启动后健康检查未通过（端口 {}）", port));
    }
    *state.gateway_pid.lock().await = Some(pid);
    *state.gateway_port.lock().await = Some(port);
    Ok(port)
}

/// 停止模型网关（进程树）。
pub(crate) async fn stop_gateway(state: &State<'_, AppState>) {
    let pid = *state.gateway_pid.lock().await;
    if let Some(p) = pid {
        oh_core::winproc::kill_tree(p);
    }
    *state.gateway_pid.lock().await = None;
    *state.gateway_port.lock().await = None;
}

/// 拉起记忆服务（面板 + 记忆 API，随应用生命周期）。
pub(crate) async fn spawn_memory_server(
    state: &State<'_, AppState>,
    bundled: &Bundled,
    ws: &Path,
    key: &str,
) -> Result<u16, String> {
    stop_memory_server(state).await;
    let block = ensure_memory_block(bundled, ws)?;
    let data_dir = block.join("data");
    if !bundled.python_available() {
        return Err("未找到内置 Python 运行时。".to_string());
    }
    let token = session_token(state).await?;
    let port = pick_free_port()?;
    let settings = state.settings.lock().await.clone();
    let log_path = data_dir.join("memory-server.log");
    let pid = spawn_uvicorn(
        bundled,
        &block,
        port,
        "backend.api.main:app",
        &[
            ("HARNESS_TOKEN", &token),
            ("HARNESS_DATA_DIR", &data_dir.to_string_lossy()),
            ("HARNESS_WORKSPACE", &ws.to_string_lossy()),
            ("TIKTOKEN_CACHE_DIR", &block.join("backend").join("services").join("encodings").to_string_lossy()),
            ("HARNESS_BASE_URL", &settings.base_url),
            ("HARNESS_MODEL", &settings.model),
            ("OH_API_KEY", key),
        ],
        &log_path,
    )?;
    if !wait_ready(port, &token).await {
        let _ = stop_memory_server(state).await;
        return Err(format!("记忆服务启动后健康检查未通过（端口 {}）", port));
    }
    *state.memory_pid.lock().await = Some(pid);
    *state.memory_port.lock().await = Some(port);
    Ok(port)
}

/// 停止记忆服务（进程树）。
pub(crate) async fn stop_memory_server(state: &State<'_, AppState>) {
    let pid = *state.memory_pid.lock().await;
    if let Some(p) = pid {
        oh_core::winproc::kill_tree(p);
    }
    *state.memory_pid.lock().await = None;
    *state.memory_port.lock().await = None;
}

/// 退出钩子：统一停止全部 sidecar。
pub(crate) fn stop_all_sync(state: &AppState) {
    if let Ok(g) = state.memory_pid.try_lock() {
        if let Some(p) = *g {
            oh_core::winproc::kill_tree(p);
        }
    }
    if let Ok(g) = state.gateway_pid.try_lock() {
        if let Some(p) = *g {
            oh_core::winproc::kill_tree(p);
        }
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
