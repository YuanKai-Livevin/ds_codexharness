//! 设置相关 Tauri 命令：设置读写、API Key、连通性测试、状态查询。

use crate::app_state::{AppState, EngineState};
use oh_core::config::AppSettings;
use oh_core::dpapi;
use oh_core::python::Bundled;
use oh_core::workspace;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize, Clone)]
pub(crate) struct StatusInfo {
    running: bool,
    engine_state: String,
    workspace: String,
    sandbox: String,
    model: String,
    provider: String,
    python_ok: bool,
    codex_ok: bool,
    base_url: String,
}

#[derive(Serialize, Clone)]
pub(crate) struct MaskedKeyInfo {
    present: bool,
    masked: String,
    provider: String,
}

#[tauri::command]
pub(crate) async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub(crate) async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
    // 校验工作区（必须是非空绝对路径的文件夹）
    let ws = workspace::ensure_workspace(&settings.workspace_path)?;
    let ws_str = ws.to_string_lossy().to_string();
    // 若引擎在运行：先停止（设置变更需重启引擎生效，前端随后自动重启）
    if state.engine_running.load(Ordering::SeqCst) {
        // 同时停止模型网关（记忆服务保持运行，供面板继续使用）
        crate::services::memory_sidecar::stop_gateway(&state).await;
        let mut guard = state.engine.lock().await;
        if let Some(server) = guard.as_mut() {
            server.stop().await;
        }
        *guard = None;
        *state.engine_pid.lock().await = None;
        *state.engine_state.lock().await = EngineState::Stopped;
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

/// 保存 API Key（DPAPI 加密后写入 settings.json）。
/// 空值视为「未填写」：保留已保存的 Key，避免设置页误清除。
#[tauri::command]
pub(crate) async fn save_api_key(state: State<'_, AppState>, api_key: String) -> Result<(), String> {
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

/// 是否已保存 API Key。
#[tauri::command]
pub(crate) async fn has_api_key(state: State<'_, AppState>) -> Result<bool, String> {
    let s = state.settings.lock().await;
    Ok(s.api_key_enc.as_deref().map(|e| !e.is_empty()).unwrap_or(false))
}

/// 当前实际使用的 API Key（脱敏显示：前 6 + 后 4，供用户确认用的哪个 Key）。
#[tauri::command]
pub(crate) async fn get_api_key_masked(state: State<'_, AppState>) -> Result<MaskedKeyInfo, String> {
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

/// 测试模型连通性：用当前（或传入）API Key 请求 models 接口。
#[tauri::command]
pub(crate) async fn test_connection(
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
    let model = settings.model.clone();
    // 能力自检（R4）：探测 /models、/responses、/chat/completions，输出能力档案 JSON
    let script = r#"import sys, json, urllib.request, urllib.error
base, model, key = sys.argv[1], sys.argv[2], sys.argv[3]
def hdr():
    h = {"Content-Type": "application/json"}
    if key and key != "EMPTY":
        h["Authorization"] = "Bearer " + key
    return h
def req(url, data=None, timeout=20):
    body = json.dumps(data).encode() if data is not None else None
    r = urllib.request.Request(url, data=body, headers=hdr(), method="POST" if data is not None else "GET")
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            return resp.status, resp.read().decode("utf-8", "replace")[:2000]
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")[:2000]
    except Exception as e:
        return 0, str(e)[:300]
rep = {"base": base, "model": model, "context_window": "unknown"}
s, _ = req(base + "/models")
rep["models_http"] = s
s, body = req(base + "/responses", {"model": model, "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}], "max_output_tokens": 1, "stream": False})
rep["supports_responses"] = s == 200
rep["supports_reasoning"] = s == 200 and '"reasoning"' in body
rep["returns_usage"] = s == 200 and '"usage"' in body
s, _ = req(base + "/chat/completions", {"model": model, "messages": [{"role": "user", "content": "hi"}], "max_tokens": 1})
rep["supports_chat"] = s == 200
if rep["supports_responses"]:
    rep["suggestion"] = "direct"
elif rep["supports_chat"]:
    rep["suggestion"] = "use_bridge"
else:
    rep["suggestion"] = "check_base"
print(json.dumps(rep, ensure_ascii=False))
"#;
    let mut cmd = std::process::Command::new(&python);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd
        .args(["-c", script, &base, &model, &key])
        .output()
        .map_err(|e| format!("无法启动 Python: {}", e))?;
    let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // 解析能力档案
    let caps: serde_json::Value = serde_json::from_str(&msg).unwrap_or_else(|_| serde_json::json!({ "parse_error": msg }));
    let models_ok = caps.get("models_http").and_then(|v| v.as_i64()).map(|v| v == 200).unwrap_or(false);
    let supports_responses = caps.get("supports_responses").and_then(|v| v.as_bool()).unwrap_or(false);
    let supports_chat = caps.get("supports_chat").and_then(|v| v.as_bool()).unwrap_or(false);
    let suggestion = caps.get("suggestion").and_then(|v| v.as_str()).unwrap_or("check_base").to_string();
    let ok = models_ok || supports_responses || supports_chat;
    let mut caps_obj = caps.clone();
    caps_obj["ok"] = serde_json::json!(ok);
    let message = if ok {
        if suggestion == "use_bridge" {
            "✅ 连接成功：仅支持 chat/completions，建议开启「内置翻译层」".to_string()
        } else if suggestion == "direct" {
            "✅ 连接成功：支持 Responses API，可直接使用".to_string()
        } else {
            "✅ 连接成功：模型服务可用".to_string()
        }
    } else {
        format!("连接失败：无法访问 {}/models、/responses、/chat/completions", base)
    };
    caps_obj["message"] = serde_json::json!(message);
    Ok(caps_obj)
}

/// 引擎与运行环境状态。
#[tauri::command]
pub(crate) async fn get_status(state: State<'_, AppState>, app: AppHandle) -> Result<StatusInfo, String> {
    let s = state.settings.lock().await.clone();
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    Ok(StatusInfo {
        running: state.engine_running.load(Ordering::SeqCst),
        engine_state: state.engine_state.lock().await.as_str().to_string(),
        workspace: s.workspace_path,
        sandbox: s.sandbox_mode,
        model: s.model,
        provider: s.provider_name,
        python_ok: bundled.python_available(),
        codex_ok: bundled.codex_available(),
        base_url: s.base_url,
    })
}
