//! 引擎生命周期与消息命令：启动/停止/发送/审批/中断/沙箱。

use crate::app_state::{data_root, AppState, EngineState, MEMORY_PORT};
use crate::services::memory_sidecar::{
    memory_block_dir, parse_input_tokens, spawn_memory_server, write_conversation_tokens,
};
use oh_core::codex::CodexServer;
use oh_core::model::EngineEvent;
use oh_core::python::Bundled;
use oh_core::{scanner, workspace};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

/// 统一状态写入：engine_state 为唯一真相源，engine_running 为镜像。
async fn set_engine_state(app: &AppHandle, s: EngineState) {
    let state = app.state::<AppState>();
    *state.engine_state.lock().await = s;
    state.engine_running.store(s.is_running(), Ordering::SeqCst);
}

/// 启动引擎（内部实现；api_key 为空时尝试使用已保存的 Key）。
/// 任何失败都会把状态置为 Failed（前端可一键重启）。
pub(crate) async fn start_engine_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    api_key: String,
) -> Result<(), String> {
    let res = start_engine_inner_impl(app, state, api_key).await;
    if res.is_err() {
        let st = *state.engine_state.lock().await;
        // 只在仍处于 Starting（未成功也未主动停止）时标记失败
        if st == EngineState::Starting {
            *state.engine_state.lock().await = EngineState::Failed;
            state.engine_running.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "oh-event",
                EngineEvent::Status {
                    state: "failed".into(),
                    detail: res.as_ref().unwrap_err().clone(),
                },
            );
        }
    }
    res
}

async fn start_engine_inner_impl(
    app: &AppHandle,
    state: &State<'_, AppState>,
    api_key: String,
) -> Result<(), String> {
    {
        let st = *state.engine_state.lock().await;
        if st.is_running() || st == EngineState::Starting || st == EngineState::Stopping {
            return Err("引擎已在运行中或正在启动/停止。".to_string());
        }
    }
    *state.engine_state.lock().await = EngineState::Starting;
    state.engine_running.store(false, Ordering::SeqCst);
    let settings = state.settings.lock().await.clone();
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());

    // 解析 API Key：优先参数，其次已保存的密文
    let mut key = api_key.trim().to_string();
    if key.is_empty() {
        if let Some(enc) = settings.api_key_enc.as_deref() {
            if !enc.is_empty() {
                key = oh_core::dpapi::decrypt(enc)
                    .map_err(|e| format!("读取已保存的 API Key 失败: {}", e))?;
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

    // 拉起记忆面板后端（记忆块管理 + 真实上下文水位 + 可选翻译层）
    // 普通模式：失败不阻塞引擎；开启翻译层（use_bridge）时引擎依赖本地 /responses 服务，
    // 必须 fail-closed —— 服务起不来就拒绝启动，避免请求发往错误/失效进程（T0-03）。
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
            if settings.use_bridge {
                return Err(format!(
                    "已开启「内置翻译层」，但本地翻译服务启动失败（{}）。已停止启动引擎，避免请求发往不可用服务。请检查端口 8765 是否被占用后重试。",
                    e
                ));
            }
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

    // 事件转发任务（同时更新状态机：Busy/Ready/崩溃 Failed + 写入记忆水位）
    let mut rx = server.take_events().ok_or("事件通道不可用")?;
    let handle = app.clone();
    let mem_data = memory_block_dir(&ws).join("data");
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match &ev {
                EngineEvent::TurnStarted { .. } => {
                    set_engine_state(&handle, EngineState::Busy).await;
                }
                EngineEvent::TurnCompleted { .. } => {
                    set_engine_state(&handle, EngineState::Ready).await;
                    if let EngineEvent::TurnCompleted { usage, .. } = &ev {
                        if let Some(tokens) = parse_input_tokens(usage) {
                            write_conversation_tokens(&mem_data, tokens).await;
                        }
                    }
                }
                EngineEvent::EngineStopped => {
                    // 非预期退出 → Failed；主动停止（Stopping）→ Stopped
                    let st = *handle.state::<AppState>().engine_state.lock().await;
                    if st == EngineState::Stopping {
                        set_engine_state(&handle, EngineState::Stopped).await;
                    } else {
                        set_engine_state(&handle, EngineState::Failed).await;
                        let _ = handle.emit(
                            "oh-event",
                            EngineEvent::Status {
                                state: "failed".into(),
                                detail: "引擎进程异常退出，可重新启动。".into(),
                            },
                        );
                    }
                }
                _ => {}
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
    *state.engine_state.lock().await = EngineState::Ready;
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
pub(crate) async fn start_engine(
    state: State<'_, AppState>,
    app: AppHandle,
    api_key: String,
) -> Result<(), String> {
    start_engine_inner(&app, &state, api_key).await
}

#[tauri::command]
pub(crate) async fn stop_engine(state: State<'_, AppState>) -> Result<(), String> {
    *state.engine_state.lock().await = EngineState::Stopping;
    let mut guard = state.engine.lock().await;
    if let Some(server) = guard.as_mut() {
        server.stop().await;
    }
    *guard = None;
    *state.engine_pid.lock().await = None;
    *state.engine_state.lock().await = EngineState::Stopped;
    state.engine_running.store(false, Ordering::SeqCst);
    Ok(())
}

/// 发送用户消息（先做越界扫描与破坏性提示）。
#[tauri::command]
pub(crate) async fn send_message(state: State<'_, AppState>, text: String) -> Result<(), String> {
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
pub(crate) async fn respond_approval(
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
pub(crate) async fn interrupt(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.engine.lock().await;
    if let Some(server) = guard.as_mut() {
        server.interrupt().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 一键配置 Windows 沙箱（通过 app-server RPC 执行，需引擎已启动）。
/// 模式跟随设置：unelevated（免 UAC）| elevated（会弹 UAC 授权）。
#[tauri::command]
pub(crate) async fn setup_sandbox(state: State<'_, AppState>) -> Result<(), String> {
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
pub(crate) async fn sandbox_status(state: State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.engine.lock().await;
    match guard.as_mut() {
        Some(server) => server.sandbox_readiness().await.map_err(|e| e.to_string()),
        None => Ok("engine-stopped".to_string()),
    }
}
