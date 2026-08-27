//! 办公自动化助手 —— Tauri 应用主体（库形式，供 main 调用）。

use oh_core::codex::CodexServer;
use oh_core::config::AppSettings;
use oh_core::dpapi;
use oh_core::model::EngineEvent;
use oh_core::python::Bundled;
use oh_core::scanner;
use oh_core::workspace;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tokio::sync::Mutex;

struct AppState {
    settings_path: PathBuf,
    codex_home: PathBuf,
    settings: Mutex<AppSettings>,
    api_key: Mutex<Option<String>>,
    engine: Mutex<Option<CodexServer>>,
    engine_pid: Mutex<Option<u32>>,
    engine_running: AtomicBool,
    memory_pid: Mutex<Option<u32>>,
}

/// 记忆面板/翻译层服务端口（与 oh-core 常量保持一致）。
const MEMORY_PORT: u16 = oh_core::MEMORY_PORT;

/// 应用数据根目录：所有应用级配置/数据集中在一个目录便于管理。
/// 包含 settings.json、codex-home（引擎）、skills（技能库）、logs（日志）。
fn data_root() -> PathBuf {
    PathBuf::from("C:\\HARNESS")
}

/// 迁移旧版数据目录（%APPDATA%\OfficeHarness）到新根目录 C:\HARNESS。
/// 只迁移尚未存在的新位置，避免覆盖用户新数据；旧目录残留由用户自行清理。
fn migrate_legacy_data_root() {
    let old_root = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OfficeHarness");
    if !old_root.exists() {
        return;
    }
    let new_root = data_root();
    let _ = std::fs::create_dir_all(&new_root);
    for name in ["settings.json", "codex-home", "skills"] {
        let src = old_root.join(name);
        let dst = new_root.join(name);
        if src.exists() && !dst.exists() {
            let _ = std::fs::rename(&src, &dst);
        }
    }
    // 日志目录旧默认值 C:\HARNESS\logs 不变；若旧目录已空则删除
    let _ = std::fs::remove_dir(&old_root);
}

#[derive(Serialize, Clone)]
struct StatusInfo {
    running: bool,
    workspace: String,
    sandbox: String,
    model: String,
    provider: String,
    python_ok: bool,
    codex_ok: bool,
    base_url: String,
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
    // 校验工作区（必须是非空绝对路径的文件夹）
    let ws = workspace::ensure_workspace(&settings.workspace_path)?;
    let ws_str = ws.to_string_lossy().to_string();
    // 若引擎在运行：先停止（设置变更需重启引擎生效，前端随后自动重启）
    if state.engine_running.load(Ordering::SeqCst) {
        let mut guard = state.engine.lock().await;
        if let Some(server) = guard.as_mut() {
            server.stop().await;
        }
        *guard = None;
        *state.engine_pid.lock().await = None;
        state.engine_running.store(false, Ordering::SeqCst);
    }
    let mut cur = state.settings.lock().await;
    // 保留已保存的 API Key 密文（前端保存的设置不包含密钥）
    let mut next = settings.clone();
    next.api_key_enc = cur.api_key_enc.clone();
    next.workspace_path = ws_str;
    // 更新最近使用的工作区（去重，最多 6 个）
    let mut recent = next.recent_workspaces.clone();
    recent.retain(|r| r != &next.workspace_path);
    recent.insert(0, next.workspace_path.clone());
    recent.truncate(6);
    next.recent_workspaces = recent;
    *cur = next.clone();
    cur.save(&state.settings_path)
}

/// 常用位置（桌面/文档/下载），供工作区快捷切换。
#[tauri::command]
async fn common_folders() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(d) = dirs::desktop_dir() {
        out.push(("桌面".to_string(), d.to_string_lossy().to_string()));
    }
    if let Some(d) = dirs::document_dir() {
        out.push(("文档".to_string(), d.to_string_lossy().to_string()));
    }
    if let Some(d) = dirs::download_dir() {
        out.push(("下载".to_string(), d.to_string_lossy().to_string()));
    }
    out
}

#[tauri::command]
async fn get_status(state: State<'_, AppState>, app: AppHandle) -> Result<StatusInfo, String> {
    let s = state.settings.lock().await.clone();
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    Ok(StatusInfo {
        running: state.engine_running.load(Ordering::SeqCst),
        workspace: s.workspace_path,
        sandbox: s.sandbox_mode,
        model: s.model,
        provider: s.provider_name,
        python_ok: bundled.python_available(),
        codex_ok: bundled.codex_available(),
        base_url: s.base_url,
    })
}

/// 保存 API Key（DPAPI 加密后写入 settings.json）。
/// 空值视为「未填写」：保留已保存的 Key，避免设置页误清除。
#[tauri::command]
async fn save_api_key(state: State<'_, AppState>, api_key: String) -> Result<(), String> {
    let mut settings = state.settings.lock().await.clone();
    let trimmed = api_key.trim().to_string();
    if trimmed.is_empty() {
        // 未填写 → 保留现有密文，不覆盖
        return Ok(());
    }
    settings.api_key_enc = Some(dpapi::encrypt(&trimmed).map_err(|e| format!("加密 API Key 失败: {}", e))?);
    settings.save(&state.settings_path)?;
    *state.settings.lock().await = settings;
    Ok(())
}

/// 测试模型连通性：用当前（或传入）API Key 请求 models 接口。
#[tauri::command]
async fn test_connection(
    state: State<'_, AppState>,
    app: AppHandle,
    api_key: String,
) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().await.clone();
    let mut key = api_key.trim().to_string();
    if key.is_empty() {
        if let Some(enc) = settings.api_key_enc.as_deref() {
            if !enc.is_empty() {
                key = dpapi::decrypt(enc).map_err(|e| e.to_string())?;
            }
        }
    }
    if key.is_empty() {
        return Ok(serde_json::json!({ "ok": false, "message": "未配置 API Key" }));
    }
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let python = bundled.python_exe();
    if !python.exists() {
        return Err("未找到内置 Python 运行时".to_string());
    }
    let base = settings.base_url.trim().trim_end_matches('/').to_string();
    let url = format!("{}/models", base);
    // 内网免密钥模式：无 Key 时不带 Authorization 头（requires_openai_auth=false）
    let script = r#"import sys, urllib.request, urllib.error
url, key = sys.argv[1], sys.argv[2]
headers = {"Content-Type": "application/json"}
if key and key != "EMPTY":
    headers["Authorization"] = "Bearer " + key
req = urllib.request.Request(url, headers=headers)
try:
    with urllib.request.urlopen(req, timeout=20) as r:
        print(r.status)
except urllib.error.HTTPError as e:
    print("HTTP", e.code)
except Exception as e:
    print("ERR", e)
"#;
    let mut cmd = std::process::Command::new(&python);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd
        .args(["-c", script, &url, &key])
        .output()
        .map_err(|e| format!("无法启动 Python: {}", e))?;
    let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if msg.starts_with("200") {
        Ok(serde_json::json!({ "ok": true, "message": "✅ 连接成功（HTTP 200），模型服务可用" }))
    } else if msg.starts_with("HTTP") {
        Ok(serde_json::json!({ "ok": false, "message": format!("连接失败：{}", msg) }))
    } else {
        Ok(serde_json::json!({ "ok": false, "message": format!("连接失败：{}", msg) }))
    }
}

/// 从工作区列表移除一个工作区。
/// 若删除的是当前工作区，自动切换到下一个可用工作区（或默认），返回新工作区路径。
/// SKILLS 仓库为固定工作区，不可删除。
#[tauri::command]
async fn remove_workspace(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let skills_repo = data_root().join("skills").to_string_lossy().to_string();
    if path == skills_repo {
        return Err("SKILLS 仓库是固定工作区，不可删除。".to_string());
    }
    let mut settings = state.settings.lock().await.clone();
    settings.recent_workspaces.retain(|w| w != &path);
    let mut switched = false;
    if settings.workspace_path == path {
        let next = settings.recent_workspaces.first().cloned();
        let fallback = dirs::document_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("办公工作区")
            .to_string_lossy()
            .to_string();
        settings.workspace_path = next.unwrap_or(fallback);
        settings.recent_workspaces.retain(|w| w != &settings.workspace_path);
        switched = true;
    }
    settings.save(&state.settings_path)?;
    let new_ws = settings.workspace_path.clone();
    *state.settings.lock().await = settings;
    Ok(if switched { new_ws } else { String::new() })
}

/// SKILLS 仓库信息（路径 + 技能列表）。
#[derive(serde::Serialize, Clone)]
struct SkillInfo {
    name: String,
    description: String,
    path: String,
}

#[derive(serde::Serialize, Clone)]
struct SkillsRepoInfo {
    path: String,
    skills: Vec<SkillInfo>,
}

/// 读取 SKILLS 仓库（固定工作区）路径与技能列表（扫描 SKILL.md）。
#[tauri::command]
async fn get_skills_repo() -> Result<SkillsRepoInfo, String> {
    let repo = data_root().join("skills");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&repo) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let md = dir.join("SKILL.md");
            if md.exists() {
                if let Ok(content) = std::fs::read_to_string(&md) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let description = parse_skill_description(&content);
                    skills.push(SkillInfo {
                        name,
                        description,
                        path: dir.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(SkillsRepoInfo {
        path: repo.to_string_lossy().to_string(),
        skills,
    })
}

/// 解析 SKILL.md 的 frontmatter description。
fn parse_skill_description(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return String::new();
    }
    let end = trimmed[3..].find("---").map(|i| i + 3).unwrap_or(trimmed.len());
    let front = &trimmed[3..end];
    for line in front.lines() {
        if let Some(v) = line.strip_prefix("description:") {
            return v.trim().trim_matches('"').trim_matches('\'').to_string();
        }
    }
    String::new()
}

/// 在资源管理器中打开 SKILLS 仓库。
#[tauri::command]
async fn open_skills_repo() -> Result<(), String> {
    let repo = data_root().join("skills");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&repo)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 从所选目录导入技能（含 SKILL.md 的文件夹复制到技能仓库），返回导入的技能名。
#[tauri::command]
async fn import_skills(dir: String) -> Result<Vec<String>, String> {
    let repo = data_root().join("skills");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    let src = std::path::Path::new(&dir);
    if !src.is_dir() {
        return Err("所选路径不是文件夹。".to_string());
    }
    let mut candidates: Vec<std::path::PathBuf> = vec![src.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                candidates.push(entry.path());
            }
        }
    }
    let mut imported = Vec::new();
    for c in candidates {
        let md = c.join("SKILL.md");
        if !md.exists() {
            continue;
        }
        let Some(name) = c.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        let dst = repo.join(&name);
        if dst.exists() {
            std::fs::remove_dir_all(&dst).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&c, &dst).map_err(|e| format!("复制 {} 失败: {}", name, e))?;
        imported.push(name);
    }
    Ok(imported)
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ==================== 记忆面板（记忆块管理后端） ====================

/// 记忆功能块在工作区内的目录：{工作区}/.harness-memory/
fn memory_block_dir(ws: &std::path::Path) -> PathBuf {
    ws.join(".harness-memory")
}

/// 把随程序分发的记忆面板代码（backend/ frontend/）部署进当前工作区。
/// 源：exe 旁 memory-block/{backend,frontend}（生产）或开发根 backend/ frontend/。
fn ensure_memory_block(bundled: &Bundled, ws: &std::path::Path) -> Result<PathBuf, String> {
    let root = &bundled.root;
    let src_backend = {
        let p = root.join("memory-block").join("backend");
        if p.exists() { p } else { root.join("backend") }
    };
    let src_frontend = {
        let p = root.join("memory-block").join("frontend");
        if p.exists() { p } else { root.join("frontend") }
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
fn copy_memory_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
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
async fn spawn_memory_server(
    _app: &AppHandle,
    state: &State<'_, AppState>,
    bundled: &Bundled,
    ws: &std::path::Path,
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
async fn stop_memory_server(state: &State<'_, AppState>) {
    let pid = *state.memory_pid.lock().await;
    if let Some(p) = pid {
        oh_core::winproc::kill_tree(p);
    }
    *state.memory_pid.lock().await = None;
}

/// 从 codex turn/completed 的 usage JSON 中提取上下文 tokens（输入 tokens）。
fn parse_input_tokens(usage: &str) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_str(usage).ok()?;
    v.get("input_tokens")
        .and_then(|x| x.as_u64())
        .or_else(|| v.pointer("/total_token_usage/input_tokens").and_then(|x| x.as_u64()))
}

/// 把 codex 每轮的真实上下文 tokens 写入记忆面板数据文件（水位监控数据源）。
async fn write_conversation_tokens(data_dir: &std::path::Path, tokens: u64) {
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

/// 记忆面板状态（供前端显示服务可用性）。
#[derive(Serialize, Clone)]
struct MemoryStatus {
    running: bool,
    port: u16,
    workspace: String,
    data_dir: String,
    panel_url: String,
    deployed: bool,
}

#[tauri::command]
async fn memory_status(state: State<'_, AppState>) -> Result<MemoryStatus, String> {
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
async fn open_memory_panel(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
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

fn chrono_like_now() -> String {
    // 轻量 ISO8601（无 chrono 依赖）：Windows 下用 SystemTime
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 仅用于展示，简单转 UTC 时间
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

/// 是否已保存 API Key。
#[tauri::command]
async fn has_api_key(state: State<'_, AppState>) -> Result<bool, String> {
    let s = state.settings.lock().await;
    Ok(s.api_key_enc.as_deref().map(|e| !e.is_empty()).unwrap_or(false))
}

/// 当前实际使用的 API Key（脱敏显示：前 6 + 后 4，供用户确认用的哪个 Key）。
#[derive(Serialize, Clone)]
struct MaskedKeyInfo {
    present: bool,
    masked: String,
    provider: String,
}

#[tauri::command]
async fn get_api_key_masked(state: State<'_, AppState>) -> Result<MaskedKeyInfo, String> {
    let s = state.settings.lock().await.clone();
    let mut info = MaskedKeyInfo {
        present: false,
        masked: String::new(),
        provider: s.provider_name.clone(),
    };
    if let Some(enc) = s.api_key_enc.as_deref() {
        if !enc.is_empty() {
            if let Ok(k) = dpapi::decrypt(enc) {
                let k = k.trim().to_string();
                if !k.is_empty() {
                    let masked = if k.len() > 10 {
                        format!("{}…{}", &k[..6], &k[k.len() - 4..])
                    } else {
                        let n = k.len().min(3);
                        format!("{}…", &k[..n])
                    };
                    info.present = true;
                    info.masked = masked;
                }
            }
        }
    }
    Ok(info)
}

/// 启动引擎（内部实现；api_key 为空时尝试使用已保存的 Key）。
async fn start_engine_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    api_key: String,
) -> Result<(), String> {
    if state.engine_running.load(Ordering::SeqCst) {
        return Err("引擎已在运行中。".to_string());
    }
    let settings = state.settings.lock().await.clone();
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());

    // 解析 API Key：优先参数，其次已保存的密文
    let mut key = api_key.trim().to_string();
    if key.is_empty() {
        if let Some(enc) = settings.api_key_enc.as_deref() {
            if !enc.is_empty() {
                key = dpapi::decrypt(enc).map_err(|e| format!("读取已保存的 API Key 失败: {}", e))?;
            }
        }
    }
    if key.is_empty() {
        if settings.no_auth {
            // 内网免密钥模式：无需 Key，用占位值让引擎跳过鉴权（requires_openai_auth=false）
            key = "EMPTY".to_string();
        } else {
            return Err("未配置 API Key。若为内网免密钥部署，请勾选「内网部署，无需 API Key」。".to_string());
        }
    }
    if !bundled.python_available() {
        return Err("未找到内置 Python 运行时（runtime/python312），请检查程序目录是否完整。".to_string());
    }
    if !bundled.codex_available() {
        return Err("未找到内置 codex 引擎（codex-bin），请检查程序目录是否完整。".to_string());
    }

    let ws = workspace::ensure_workspace(&settings.workspace_path)?;
    let ws_str = ws.to_string_lossy().to_string();

    // 准备 CODEX_HOME（config.toml + 审批规则）
    CodexServer::prepare_home(&state.codex_home, &settings)?;

    // 确保 SKILLS 仓库目录存在
    let skills_repo = data_root().join("skills");
    std::fs::create_dir_all(&skills_repo).map_err(|e| format!("无法创建技能仓库: {}", e))?;
    let skills_repo_str = skills_repo.to_string_lossy().to_string();

    // 拉起记忆面板后端（记忆块管理 + 真实上下文水位），失败不阻塞引擎
    match spawn_memory_server(app, state, &bundled, &ws, &key).await {
        Ok(pid) => {
            let _ = app.emit(
                "oh-event",
                EngineEvent::Log {
                    level: "info".into(),
                    msg: format!("记忆面板服务已启动（127.0.0.1:{}，pid {}）", MEMORY_PORT, pid),
                },
            );
        }
        Err(e) => {
            let _ = app.emit(
                "oh-event",
                EngineEvent::Log {
                    level: "warn".into(),
                    msg: format!("记忆面板服务启动失败：{}", e),
                },
            );
        }
    }

    let mut server = CodexServer::spawn(&bundled, &state.codex_home, &settings, &key)
        .await
        .map_err(|e| format!("启动引擎失败: {}", e))?;

    server
        .initialize()
        .await
        .map_err(|e| format!("初始化引擎失败: {}", e))?;

    // 注册 SKILLS 仓库为 codex 额外技能根
    let _ = server.register_skills_roots(&[skills_repo_str.clone()]).await;

    let thread_id = server
        .start_thread(&ws_str, &settings.sandbox_mode, &settings.model, &skills_repo_str)
        .await
        .map_err(|e| format!("创建会话失败: {}", e))?;

    // 事件转发任务（同时把真实上下文 tokens 写入记忆面板水位文件）
    let mut rx = server.take_events().ok_or("事件通道不可用")?;
    let handle = app.clone();
    let mem_data = memory_block_dir(&ws).join("data");
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let EngineEvent::TurnCompleted { usage, .. } = &ev {
                if let Some(tokens) = parse_input_tokens(usage) {
                    write_conversation_tokens(&mem_data, tokens).await;
                }
            }
            let _ = handle.emit("oh-event", ev);
        }
    });

    // 自动配置 Windows 沙箱：若未就绪则静默执行（unelevated 免 UAC）
    let sb_mode = settings.windows_sandbox.clone();
    let sb_ws = ws_str.clone();
    let sb_log = app.clone();
    let readiness = server.sandbox_readiness().await.unwrap_or_else(|_| "unknown".to_string());
    if readiness != "ready" {
        let _ = server.setup_windows_sandbox_mode(&sb_ws, &sb_mode).await;
        let _ = sb_log.emit(
            "oh-event",
            EngineEvent::Log {
                level: "info".into(),
                msg: format!("Windows 沙箱状态 {}，已自动发起配置（{}）", readiness, sb_mode),
            },
        );
    }

    *state.api_key.lock().await = Some(key);
    *state.engine_pid.lock().await = server.pid();
    *state.engine.lock().await = Some(server);
    state.engine_running.store(true, Ordering::SeqCst);
    let _ = app.emit(
        "oh-event",
        EngineEvent::Log {
            level: "info".into(),
            msg: format!("引擎已就绪，会话 {}，工作区：{}", thread_id, ws_str),
        },
    );
    Ok(())
}

/// 启动引擎：拉起 codex app-server 并建立会话。
#[tauri::command]
async fn start_engine(
    state: State<'_, AppState>,
    app: AppHandle,
    api_key: String,
) -> Result<(), String> {
    start_engine_inner(&app, &state, api_key).await
}

#[tauri::command]
async fn stop_engine(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.engine.lock().await;
    if let Some(server) = guard.as_mut() {
        server.stop().await;
    }
    *guard = None;
    *state.engine_pid.lock().await = None;
    state.engine_running.store(false, Ordering::SeqCst);
    Ok(())
}

/// 发送用户消息（先做越界扫描与破坏性提示）。
#[tauri::command]
async fn send_message(state: State<'_, AppState>, text: String) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("消息不能为空".to_string());
    }
    // 1) 越界扫描：拒绝并警告
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let escapes = workspace::scan_text_for_escapes(&text, Some(&ws));
    if !escapes.is_empty() {
        let msgs: Vec<String> = escapes.iter().map(|e| e.message.clone()).collect();
        return Err(format!("【安全警告】已拒绝执行：{}", msgs.join(" ")));
    }
    // 2) 破坏性关键词：追加提示，让 agent 先出计划
    let destructive = scanner::classify_instruction(&text);
    let mut payload = text.clone();
    if !destructive.is_empty() {
        let labels: Vec<String> = destructive.iter().map(|m| m.label.clone()).collect();
        payload.push_str(&format!(
            "\n（提醒：该任务涉及 {}，请严格按流程先输出【执行计划】与【危险操作警告】并等待用户批准。）",
            labels.join("、")
        ));
    }
    let mut guard = state.engine.lock().await;
    let server = guard
        .as_mut()
        .ok_or_else(|| "引擎未启动，请先启动引擎。".to_string())?;
    server
        .send_turn(&payload)
        .await
        .map_err(|e| format!("发送失败: {}", e))?;
    Ok(())
}

/// 响应审批请求。
#[tauri::command]
async fn respond_approval(
    state: State<'_, AppState>,
    request_id: i64,
    decision: String,
) -> Result<(), String> {
    let mut guard = state.engine.lock().await;
    let server = guard
        .as_mut()
        .ok_or_else(|| "引擎未启动".to_string())?;
    server
        .respond_approval(request_id, &decision)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn interrupt(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.engine.lock().await;
    if let Some(server) = guard.as_mut() {
        server.interrupt().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn open_workspace(state: State<'_, AppState>) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    if !ws.is_dir() {
        return Err("工作区路径不是文件夹，请到「设置」中重新选择。".to_string());
    }
    std::process::Command::new("explorer")
        .arg(&ws)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 原生文件夹选择（rfd：Rust 原生 IFileDialog，仅允许选择文件夹）。
#[tauri::command]
async fn pick_folder() -> Result<Option<String>, String> {
    let handle = tauri::async_runtime::spawn(async move {
        rfd::AsyncFileDialog::new()
            .set_title("选择工作区文件夹（请点底部「选择文件夹」按钮）")
            .pick_folder()
            .await
    });
    let folder = handle
        .await
        .map_err(|e| format!("文件夹选择器异常: {}", e))?;
    match folder {
        Some(f) => {
            let p = f.path().to_string_lossy().to_string();
            // 二次校验：必须是文件夹
            if std::path::Path::new(&p).is_file() {
                Err("所选路径是一个文件，请重新选择一个文件夹。".to_string())
            } else {
                Ok(Some(p))
            }
        }
        None => Ok(None),
    }
}

#[derive(serde::Serialize, Clone)]
struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: String,
}

/// 列出工作区内目录（严格限制在工作区范围）。
#[tauri::command]
async fn list_dir(state: State<'_, AppState>, path: String) -> Result<Vec<FileEntry>, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许浏览工作区目录内的文件。".to_string());
    }
    if !target.is_dir() {
        return Err("指定路径不是目录。".to_string());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(target).map_err(|e| e.to_string())? {
        let Ok(e) = entry else { continue };
        let md = e.metadata().ok();
        out.push(FileEntry {
            name: e.file_name().to_string_lossy().to_string(),
            is_dir: md.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            size: md.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: md
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
                .unwrap_or(0)
                .to_string(),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// 递归列出工作区内全部文件（相对路径），供 @ 引用选择。限制数量避免超大工作区卡顿。
#[tauri::command]
async fn list_workspace_files(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let mut out = Vec::new();
    let mut stack = vec![ws.clone()];
    let max = 800usize;
    while let Some(dir) = stack.pop() {
        if out.len() >= max {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if out.len() >= max {
                break;
            }
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if md.is_dir() {
                // 跳过隐藏目录与临时目录
                if name.starts_with('.') || name == "node_modules" || name == ".oh_tmp" {
                    continue;
                }
                stack.push(entry.path());
            } else if md.is_file() {
                if name.starts_with('.') {
                    continue;
                }
                let rel = entry
                    .path()
                    .strip_prefix(&ws)
                    .unwrap_or(entry.path().as_path())
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 在工作区内的路径上执行系统操作：打开（默认程序）或“在文件夹中显示”。
#[tauri::command]
async fn open_path(state: State<'_, AppState>, path: String, reveal: bool) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许操作工作区目录内的文件。".to_string());
    }
    if !target.exists() {
        return Err("文件不存在：".to_string() + &path);
    }
    if reveal {
        // 在文件夹中显示（高亮该文件）
        let mut cmd = std::process::Command::new("explorer");
        cmd.arg("/select,").arg(&target);
        cmd.spawn().map_err(|e| e.to_string())?;
    } else if target.is_dir() {
        // 文件夹：explorer 直接打开
        std::process::Command::new("explorer")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    } else {
        // 文件：用系统默认程序打开（Start-Process，避免 explorer 弹「选择打开方式」）
        let quoted = target.to_string_lossy().replace('\'', "''");
        let mut cmd = std::process::Command::new("powershell");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
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
    }
    Ok(())
}

/// 一键配置 Windows 沙箱（通过 app-server RPC 执行，需引擎已启动）。
/// 模式跟随设置：unelevated（免 UAC）| elevated（会弹 UAC 授权）。
#[tauri::command]
async fn setup_sandbox(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.engine.lock().await;
    let server = guard
        .as_mut()
        .ok_or_else(|| "引擎未启动，请先启动引擎后再配置沙箱。".to_string())?;
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    server
        .setup_windows_sandbox_mode(&ws.to_string_lossy(), &s.windows_sandbox)
        .await
        .map_err(|e| format!("发起沙箱配置失败: {}", e))
}

/// 查询 Windows 沙箱就绪状态。
#[tauri::command]
async fn sandbox_status(state: State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.engine.lock().await;
    match guard.as_mut() {
        Some(server) => server.sandbox_readiness().await.map_err(|e| e.to_string()),
        None => Ok("engine-stopped".to_string()),
    }
}

/// 列出当前工作区下的会话（任务历史）。
#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<oh_core::codex::ThreadInfo>, String> {
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
async fn current_session(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let guard = state.engine.lock().await;
    match guard.as_ref() {
        Some(server) => Ok(server.current_thread_id().await),
        None => Ok(None),
    }
}

/// 新建会话。
#[tauri::command]
async fn new_session(state: State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.engine.lock().await;
    let server = guard.as_mut().ok_or_else(|| "引擎未启动".to_string())?;
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let skills_repo = data_root().join("skills").to_string_lossy().to_string();
    let tid = server
        .start_thread(&ws.to_string_lossy(), &s.sandbox_mode, &s.model, &skills_repo)
        .await
        .map_err(|e| format!("新建会话失败: {}", e))?;
    // 新会话 = 全新上下文：复位记忆面板水位（tokens/round 归零）
    reset_memory_conversation(&memory_block_dir(&ws).join("data")).await;
    Ok(tid)
}

/// 复位记忆面板的对话水位文件（新会话/新阶段时调用）。
async fn reset_memory_conversation(data_dir: &std::path::Path) {
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

/// 切换到指定会话。
#[tauri::command]
async fn switch_session(state: State<'_, AppState>, thread_id: String) -> Result<(), String> {
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
async fn delete_session(state: State<'_, AppState>, thread_id: String) -> Result<String, String> {
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
            .start_thread(&ws.to_string_lossy(), &s.sandbox_mode, &s.model, &skills_repo)
            .await
            .map_err(|e| format!("新建会话失败: {}", e))?;
        Ok(tid)
    } else {
        Ok(String::new())
    }
}

/// 读取会话历史消息（用户/助手文本）。
#[tauri::command]
async fn session_history(
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
async fn list_tmp(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let tmp = ws.join(".oh_tmp");
    if !tmp.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&tmp).map_err(|e| e.to_string())? {
        if let Ok(e) = entry {
            out.push(e.file_name().to_string_lossy().to_string());
        }
    }
    Ok(out)
}

/// LibreOffice 是否已内置。
#[tauri::command]
async fn libreoffice_status(app: AppHandle) -> Result<bool, String> {
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    Ok(bundled.libreoffice_available())
}

/// 用 LibreOffice 打开工作区内的文件。
#[tauri::command]
async fn open_in_libreoffice(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许操作工作区目录内的文件。".to_string());
    }
    if !target.exists() {
        return Err("文件不存在：".to_string() + &path);
    }
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let soffice = bundled.soffice_exe();
    if !soffice.exists() {
        return Err("未找到内置 LibreOffice（LibreOffice 目录缺失）。".to_string());
    }
    // 用 LibreOffice 打开（GUI）
    let mut cmd = std::process::Command::new(&soffice);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.arg("--norestore").arg(&target);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// 用 LibreOffice headless 将工作区文件转换为目标格式（pdf/docx/xlsx/odt/ods...）。
/// 输出到工作区下 .oh_convert 目录，返回输出文件绝对路径。
#[tauri::command]
async fn convert_office(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    to_format: String,
) -> Result<String, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许操作工作区目录内的文件。".to_string());
    }
    if !target.exists() {
        return Err("文件不存在：".to_string() + &path);
    }
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let soffice = bundled.soffice_exe();
    if !soffice.exists() {
        return Err("未找到内置 LibreOffice。".to_string());
    }
    let fmt = to_format.trim().trim_start_matches('.').to_lowercase();
    if fmt.is_empty() {
        return Err("请指定目标格式（如 pdf / docx / xlsx / odt / ods）。".to_string());
    }
    let out_dir = ws.join(".oh_convert");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let out_dir_str = out_dir.to_string_lossy().to_string();
    let mut cmd = std::process::Command::new(&soffice);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.arg("--headless")
        .arg("--convert-to")
        .arg(&fmt)
        .arg("--outdir")
        .arg(&out_dir_str)
        .arg(&target);
    let out = cmd.output().map_err(|e| format!("启动 LibreOffice 失败: {}", e))?;
    // 输出文件：与源文件同名，扩展名改为目标格式
    let stem = target.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let result = out_dir.join(format!("{}.{}", stem, fmt));
    if !result.exists() {
        let log = String::from_utf8_lossy(&out.stderr);
        let brief: Vec<&str> = log.lines().take(6).collect();
        return Err(format!("转换失败：{}", brief.join(" | ")));
    }
    Ok(result.to_string_lossy().to_string())
}

/// 清理 .oh_tmp 下的临时文件（破坏性操作，前端需二次确认）。
#[tauri::command]
async fn cleanup_tmp(state: State<'_, AppState>) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let tmp = ws.join(".oh_tmp");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn run() {
    // 旧数据目录（%APPDATA%\OfficeHarness）自动迁移到 C:\HARNESS
    migrate_legacy_data_root();
    let root = data_root();
    let settings_path = root.join("settings.json");
    let codex_home = root.join("codex-home");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            settings_path: settings_path.clone(),
            codex_home,
            settings: Mutex::new(AppSettings::load(&settings_path)),
            api_key: Mutex::new(None),
            engine: Mutex::new(None),
            engine_pid: Mutex::new(None),
            engine_running: AtomicBool::new(false),
            memory_pid: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            save_api_key,
            has_api_key,
            get_api_key_masked,
            remove_workspace,
            test_connection,
            get_skills_repo,
            open_skills_repo,
            import_skills,
            get_status,
            libreoffice_status,
            open_in_libreoffice,
            convert_office,
            start_engine,
            stop_engine,
            send_message,
            respond_approval,
            interrupt,
            open_workspace,
            pick_folder,
            common_folders,
            list_dir,
            list_workspace_files,
            open_path,
            setup_sandbox,
            sandbox_status,
            memory_status,
            open_memory_panel,
            list_sessions,
            current_session,
            new_session,
            switch_session,
            delete_session,
            session_history,
            list_tmp,
            cleanup_tmp,
        ])
        .setup(|app| {
            // 启动时自动拉起引擎（若已配置 API Key）
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let s = state.settings.lock().await.clone();
                let has_key = s
                    .api_key_enc
                    .as_deref()
                    .map(|e| !e.is_empty())
                    .unwrap_or(false);
                if s.onboarded && (has_key || s.no_auth) {
                    let _ = handle.emit(
                        "oh-event",
                        EngineEvent::Status {
                            state: "starting".into(),
                            detail: "正在自动启动引擎…".into(),
                        },
                    );
                    if let Err(e) = start_engine_inner(&handle, &state, String::new()).await {
                        let _ = handle.emit(
                            "oh-event",
                            EngineEvent::Status {
                                state: "error".into(),
                                detail: e.clone(),
                            },
                        );
                        let _ = handle.emit(
                            "oh-event",
                            EngineEvent::Log {
                                level: "error".into(),
                                msg: e,
                            },
                        );
                    } else {
                        let _ = handle.emit(
                            "oh-event",
                            EngineEvent::Status {
                                state: "running".into(),
                                detail: "引擎已自动启动".into(),
                            },
                        );
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 退出时强制终止引擎与记忆面板子进程，避免孤儿进程
            if let RunEvent::Exit = event {
                let state = app.state::<AppState>();
                let lock = state.engine_pid.try_lock();
                let pid = match lock {
                    Ok(g) => *g,
                    Err(_) => None,
                };
                if let Some(pid) = pid {
                    let mut cmd = std::process::Command::new("taskkill");
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW，避免退出时闪黑窗口
                    }
                    let _ = cmd.args(["/PID", &pid.to_string(), "/F", "/T"]).output();
                }
                let mlock = state.memory_pid.try_lock();
                let mpid = match mlock {
                    Ok(g) => *g,
                    Err(_) => None,
                };
                if let Some(pid) = mpid {
                    oh_core::winproc::kill_tree(pid);
                }
            }
        });
}
