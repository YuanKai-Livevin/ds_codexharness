//! 引擎生命周期与消息命令：启动/停止/发送/审批/中断/沙箱。

use crate::app_state::{data_root, AppState, EngineState, TaskCtx};
use crate::commands::audit::now_ms;
use crate::services::audit::{estimate_cost, parse_usage_tokens, redact};
use crate::services::memory_sidecar::{
    memory_block_dir, parse_input_tokens, session_token, spawn_gateway, spawn_memory_server,
    stop_gateway, write_conversation_tokens,
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
        // R6 审计：启动失败
        state.audit.record(
            None,
            "error",
            "engine_start_failed",
            serde_json::json!({ "msg": redact(res.as_ref().unwrap_err()) }),
        );
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
    // T0-05：新启动清除上次的取消标志（并发 start 已由 Starting 状态互斥）
    state.start_cancel.store(false, Ordering::SeqCst);
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

    // 1) 先拉起记忆服务（面板，独立进程/随机端口/令牌），失败不阻塞引擎
    match spawn_memory_server(state, &bundled, &ws, &key).await {
        Ok(port) => {
            let _ = app.emit(
                "oh-event",
                EngineEvent::Log {
                    level: "info".into(),
                    msg: format!("记忆服务已启动（127.0.0.1:{}）", port),
                },
            );
        }
        Err(e) => {
            let _ = app.emit(
                "oh-event",
                EngineEvent::Log {
                    level: "warn".into(),
                    msg: format!("记忆服务启动失败：{}", e),
                },
            );
        }
    }

    // 2) 开启翻译层时拉起独立模型网关（随引擎生命周期），失败 fail-closed
    let mut gateway_port: Option<u16> = None;
    if settings.use_bridge {
        match spawn_gateway(state, &bundled, &ws, &key).await {
            Ok(port) => {
                gateway_port = Some(port);
                let _ = app.emit(
                    "oh-event",
                    EngineEvent::Log {
                        level: "info".into(),
                        msg: format!("模型网关已启动（127.0.0.1:{}）", port),
                    },
                );
            }
            Err(e) => {
                return Err(format!(
                    "已开启「内置翻译层」，但本地模型网关启动失败（{}）。已停止启动引擎，避免请求发往不可用服务。",
                    e
                ));
            }
        }
    }

    // 3) 准备 CODEX_HOME（config.toml + 审批规则），桥接模式 base_url 指向网关
    CodexServer::prepare_home(&state.codex_home, &settings, gateway_port)?;

    // 确保 SKILLS 仓库目录存在，并种子内置 office-tools 技能（R10，仅首次）
    let skills_repo = data_root().join("skills");
    std::fs::create_dir_all(&skills_repo).map_err(|e| format!("无法创建技能仓库: {}", e))?;
    seed_office_tools_skill(&bundled, &skills_repo);
    let skills_repo_str = skills_repo.to_string_lossy().to_string();

    // 4) 桥接模式下，引擎的 API Key = 本地网关会话令牌（网关据此鉴权）
    let engine_key = if settings.use_bridge {
        session_token(state).await?
    } else {
        key.clone()
    };

    // T0-05：启动取消检查（用户在 Starting 期间点停止）
    if state.start_cancel.swap(false, Ordering::SeqCst) {
        abort_startup(state).await;
        return Err("启动已取消".to_string());
    }

    // R6 审计：引擎启动记录（模型/网关/工作区；不含 Key）
    state.audit.record(
        None,
        "engine",
        "engine_start",
        serde_json::json!({
            "model": settings.model,
            "base_url": settings.base_url,
            "use_bridge": settings.use_bridge,
            "no_auth": settings.no_auth,
            "workspace": ws_str,
            "gateway_port": gateway_port,
        }),
    );

    let mut server = match CodexServer::spawn(&bundled, &state.codex_home, &settings, &engine_key).await {
        Ok(s) => s,
        Err(e) => {
            abort_startup(state).await;
            return Err(format!("启动引擎失败: {}", e));
        }
    };

    // T0-05：spawn 后再查一次取消（initialize 可能较慢，用户在等待时可取消）
    if state.start_cancel.swap(false, Ordering::SeqCst) {
        let _ = server.stop().await;
        abort_startup(state).await;
        return Err("启动已取消".to_string());
    }

    if let Err(e) = server.initialize().await {
        let _ = server.stop().await;
        abort_startup(state).await;
        return Err(format!("初始化引擎失败: {}", e));
    }

    // 注册 SKILLS 仓库为 codex 额外技能根
    let _ = server.register_skills_roots(&[skills_repo_str.clone()]).await;

    let thread_id = match server
        .start_thread(&ws_str, &settings.sandbox_mode, &settings.model, &skills_repo_str)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            let _ = server.stop().await;
            abort_startup(state).await;
            return Err(format!("创建会话失败: {}", e));
        }
    };

    // 事件转发任务（同时更新状态机：Busy/Ready/崩溃 Failed + 写入记忆水位 + R6 审计）
    let mut rx = server.take_events().ok_or("事件通道不可用")?;
    let handle = app.clone();
    let mem_data = memory_block_dir(&ws).join("data");
    let audit_model = settings.model.clone();
    let audit_gateway = if settings.use_bridge {
        gateway_port.map(|p| format!("127.0.0.1:{}", p))
    } else {
        None
    };
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            // ---- R6 审计挂钩 ----
            let st = handle.state::<AppState>();
            match &ev {
                EngineEvent::TurnStarted { turn_id } => {
                    let task_id = turn_id.clone();
                    let (goal, gateway, thread_id) = {
                        let mut ct = st.current_task.lock().await;
                        match ct.as_mut() {
                            Some(ctx) => {
                                ctx.task_id = Some(task_id.clone());
                                (ctx.goal.clone(), ctx.gateway.clone(), ctx.thread_id.clone())
                            }
                            None => {
                                // 兜底：无 send_message 上下文（极少见）
                                *ct = Some(TaskCtx {
                                    task_id: Some(task_id.clone()),
                                    goal: String::new(),
                                    started_ms: now_ms(),
                                    model: audit_model.clone(),
                                    workspace: String::new(),
                                    gateway: audit_gateway.clone(),
                                    thread_id: None,
                                    files: Vec::new(),
                                });
                                (String::new(), audit_gateway.clone(), None)
                            }
                        }
                    };
                    st.audit.record(
                        Some(&task_id),
                        "task",
                        "task_start",
                        serde_json::json!({
                            "goal": goal,
                            "model": audit_model,
                            "workspace": audit_workspace_or(&st).await,
                            "gateway": gateway,
                            "thread_id": thread_id,
                        }),
                    );
                }
                EngineEvent::CommandCompleted { command, status, output, .. } => {
                    let tid = current_task_id(&st).await;
                    st.audit.record(
                        tid.as_deref(),
                        "tool",
                        "command_completed",
                        serde_json::json!({
                            "command": redact(command),
                            "status": status,
                            "output_chars": output.len(),
                        }),
                    );
                }
                EngineEvent::FileChangeStarted { summary, .. } => {
                    if let Some(ctx) = st.current_task.lock().await.as_mut() {
                        if !summary.trim().is_empty() {
                            ctx.files.push(summary.clone());
                        }
                    }
                    let tid = current_task_id(&st).await;
                    st.audit.record(
                        tid.as_deref(),
                        "file",
                        "file_changed",
                        serde_json::json!({ "summary": redact(summary) }),
                    );
                }
                EngineEvent::ApprovalRequest { request_id, kind, command, reason, changes, .. } => {
                    let tid = current_task_id(&st).await;
                    st.audit.record(
                        tid.as_deref(),
                        "approval",
                        "approval_request",
                        serde_json::json!({
                            "request_id": request_id,
                            "kind": kind,
                            "command": redact(command),
                            "reason": reason,
                            "changes": changes.chars().take(400).collect::<String>(),
                        }),
                    );
                }
                EngineEvent::ApprovalResolved { request_id } => {
                    let tid = current_task_id(&st).await;
                    st.audit.record(
                        tid.as_deref(),
                        "approval",
                        "approval_closed",
                        serde_json::json!({ "request_id": request_id }),
                    );
                }
                EngineEvent::TurnCompleted { status, usage } => {
                    let (tin, tout) = parse_usage_tokens(usage);
                    let (tid, started, files, model) = {
                        let mut ct = st.current_task.lock().await;
                        match ct.take() {
                            Some(ctx) => (
                                ctx.task_id.unwrap_or_default(),
                                ctx.started_ms,
                                ctx.files,
                                ctx.model,
                            ),
                            None => (String::new(), now_ms(), Vec::new(), audit_model.clone()),
                        }
                    };
                    let duration = if started > 0 { now_ms() - started } else { 0 };
                    let cost = estimate_cost(&model, tin, tout);
                    st.audit.record_full(
                        if tid.is_empty() { None } else { Some(&tid) },
                        "task",
                        "task_end",
                        serde_json::json!({
                            "status": status,
                            "model": model,
                            "files": files,
                        }),
                        tin,
                        tout,
                        Some(duration),
                        cost,
                        None,
                    );
                }
                EngineEvent::EngineStopped => {
                    let st_state = *st.engine_state.lock().await;
                    if st_state != EngineState::Stopping {
                        st.audit.record(
                            None,
                            "error",
                            "engine_crashed",
                            serde_json::json!({ "msg": "引擎进程异常退出" }),
                        );
                    }
                }
                _ => {}
            }
            // ---- 状态机 ----
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
                        // T0-05：引擎崩溃 → 网关随引擎生命周期清理（避免孤儿网关进程）
                        let ast = handle.state::<AppState>();
                        crate::services::memory_sidecar::stop_gateway(&ast).await;
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

/// 当前任务 id（审计用）。
async fn current_task_id(st: &State<'_, AppState>) -> Option<String> {
    st.current_task
        .lock()
        .await
        .as_ref()
        .and_then(|c| c.task_id.clone())
}

/// 当前工作区（审计用；任务上下文缺失时取设置值）。
async fn audit_workspace_or(st: &State<'_, AppState>) -> String {
    let cur = st.current_task.lock().await;
    if let Some(ctx) = cur.as_ref() {
        if !ctx.workspace.is_empty() {
            return ctx.workspace.clone();
        }
    }
    let s = st.settings.lock().await.clone();
    s.workspace_path
}

/// T0-05：启动事务失败/取消时统一清理：杀模型网关、清引擎槽位。
/// 记忆服务保留（面板可用，失败不阻塞策略）；engine_state 由调用方置 Failed/Stopped。
async fn abort_startup(state: &State<'_, AppState>) {
    crate::services::memory_sidecar::stop_gateway(state).await;
    *state.engine.lock().await = None;
    *state.engine_pid.lock().await = None;
}

/// 种子内置 office-tools 技能到技能仓库：缺失或与内置版本内容不同时刷新（内置技能随版本升级）。
fn seed_office_tools_skill(bundled: &Bundled, skills_repo: &std::path::Path) {
    let src = bundled.office_tools_dir();
    if !src.join("SKILL.md").exists() || !src.join("otools.py").exists() {
        return;
    }
    let dst = skills_repo.join("office-tools");
    let need_update = if !dst.join("SKILL.md").exists() {
        true
    } else {
        // 内置技能与部署副本内容不同 → 刷新（内置技能随版本升级，用户误改会被版本覆盖）
        match (std::fs::read(src.join("SKILL.md")), std::fs::read(dst.join("SKILL.md"))) {
            (Ok(a), Ok(b)) => a != b,
            _ => true,
        }
    };
    if !need_update {
        return;
    }
    if dst.exists() {
        let _ = std::fs::remove_dir_all(&dst);
    }
    if let Err(e) = copy_dir_recursive(&src, &dst) {
        eprintln!("seed office-tools skill failed: {}", e);
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            // 跳过 Python 缓存
            if entry.file_name() == "__pycache__" {
                continue;
            }
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
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
    let cur = *state.engine_state.lock().await;
    if cur == EngineState::Starting {
        // T0-05：取消启动中 —— 置取消标志，由启动事务在步骤间检查并清理
        state.start_cancel.store(true, Ordering::SeqCst);
        // 双保险：立即清理已登记的网关，并复位状态（启动事务检测到标志后不会再覆盖）
        stop_gateway(&state).await;
        *state.engine.lock().await = None;
        *state.engine_pid.lock().await = None;
        *state.engine_state.lock().await = EngineState::Stopped;
        state.engine_running.store(false, Ordering::SeqCst);
        state.audit.record(None, "engine", "engine_start_cancelled", serde_json::json!({}));
        return Ok(());
    }
    *state.engine_state.lock().await = EngineState::Stopping;
    // 停止模型网关（随引擎生命周期；记忆服务保持运行，供面板继续使用）
    stop_gateway(&state).await;
    let mut guard = state.engine.lock().await;
    if let Some(server) = guard.as_mut() {
        server.stop().await;
    }
    *guard = None;
    *state.engine_pid.lock().await = None;
    *state.engine_state.lock().await = EngineState::Stopped;
    state.engine_running.store(false, Ordering::SeqCst);
    // R6 审计：引擎停止
    state.audit.record(None, "engine", "engine_stop", serde_json::json!({}));
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
    // R6 审计：记录任务上下文（目标/模型/工作区/网关/会话），turn_id 在 TurnStarted 时落定
    {
        let gateway = {
            let p = *state.gateway_port.lock().await;
            p.map(|p| format!("127.0.0.1:{}", p))
        };
        let thread_id = server.current_thread_id().await;
        let mut ct = state.current_task.lock().await;
        *ct = Some(TaskCtx {
            task_id: None,
            goal: payload.clone(),
            started_ms: now_ms(),
            model: s.model.clone(),
            workspace: ws.to_string_lossy().to_string(),
            gateway,
            thread_id,
            files: Vec::new(),
        });
    }
    if let Err(e) = server.send_turn(&payload).await {
        // R6 审计：发送失败（错误与重试）
        let tid = current_task_id(&state).await;
        state.audit.record(
            tid.as_deref(),
            "error",
            "send_failed",
            serde_json::json!({ "msg": redact(&e.to_string()) }),
        );
        let _ = state.current_task.lock().await.take();
        return Err(format!("发送失败: {}", e));
    }
    Ok(())
}

/// 响应审批请求。
#[tauri::command]
pub(crate) async fn respond_approval(
    state: State<'_, AppState>,
    request_id: i64,
    decision: String,
) -> Result<(), String> {
    let tid = current_task_id(&state).await;
    let res = {
        let mut guard = state.engine.lock().await;
        let server = guard
            .as_mut()
            .ok_or_else(|| "引擎未启动".to_string())?;
        server
            .respond_approval(request_id, &decision)
            .await
            .map_err(|e| e.to_string())
    };
    // R6 审计：审批决策（用户是否允许）
    match &res {
        Ok(()) => state.audit.record(
            tid.as_deref(),
            "approval",
            "approval_decision",
            serde_json::json!({ "request_id": request_id, "decision": decision }),
        ),
        Err(e) => state.audit.record(
            tid.as_deref(),
            "error",
            "approval_failed",
            serde_json::json!({ "request_id": request_id, "msg": redact(e) }),
        ),
    }
    res
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
