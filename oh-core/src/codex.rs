//! codex app-server 驱动：JSON-RPC 2.0 over stdio。

use crate::config::AppSettings;
use crate::model::EngineEvent;
use crate::python::Bundled;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum CodexError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("引擎未运行")]
    NotRunning,
    #[error("引擎已初始化")]
    AlreadyInitialized,
    #[error("RPC 错误: {0}")]
    Rpc(String),
    #[error("等待响应超时")]
    Timeout,
    #[error("{0}")]
    Other(String),
}

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, CodexError>>>>>;

/// 会话概要信息。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThreadInfo {
    pub id: String,
    pub preview: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 历史消息（用于渲染会话历史）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryMessage {
    pub role: String, // user | assistant
    pub text: String,
}

pub struct CodexServer {
    child: Option<Child>,
    out_tx: mpsc::UnboundedSender<String>,
    events: Option<mpsc::UnboundedReceiver<EngineEvent>>,
    next_id: Arc<AtomicI64>,
    pending: PendingMap,
    server_requests: Arc<Mutex<HashMap<i64, String>>>,
    thread_id: Arc<Mutex<Option<String>>>,
    turn_id: Arc<Mutex<Option<String>>>,
    running: Arc<AtomicBool>,
}

impl CodexServer {
    /// 生成 CODEX_HOME 下的 config.toml 与审批规则。
    pub fn prepare_home(codex_home: &Path, settings: &AppSettings) -> Result<(), String> {
        std::fs::create_dir_all(codex_home.join("rules")).map_err(|e| e.to_string())?;
        // 内网免密钥模式：requires_openai_auth=false，引擎不校验 Key
        let no_auth_line = if settings.no_auth {
            "requires_openai_auth = false\n"
        } else {
            ""
        };
        // 启用内置翻译层时，引擎 base_url 指向本地翻译服务（/responses → /chat/completions）
        let engine_base = if settings.use_bridge {
            format!("http://127.0.0.1:{}/", crate::MEMORY_PORT)
        } else {
            settings.base_url.clone()
        };
        let cfg = format!(
            r#"model = "{}"
model_provider = "{}"
approval_policy = "on-request"
sandbox_mode = "{}"

[windows]
sandbox = "{}"

[model_providers.{}]
name = "{}"
base_url = "{}"
wire_api = "responses"
env_key = "{}"
{}"#,
            settings.model,
            settings.provider_name,
            settings.sandbox_mode,
            if settings.windows_sandbox == "elevated" { "elevated" } else { "unelevated" },
            settings.provider_name,
            settings.provider_name,
            engine_base,
            settings.api_key_env,
            no_auth_line,
        );
        std::fs::write(codex_home.join("config.toml"), cfg).map_err(|e| e.to_string())?;

        let policy = r#"# 办公助手审批规则：破坏性操作一律要求人工确认
prefix_rule(pattern=["rm"], decision="prompt")
prefix_rule(pattern=["rmdir"], decision="prompt")
prefix_rule(pattern=["del"], decision="prompt")
prefix_rule(pattern=["erase"], decision="prompt")
prefix_rule(pattern=["rd"], decision="prompt")
prefix_rule(pattern=["Remove-Item"], decision="prompt")
prefix_rule(pattern=["format"], decision="prompt")
prefix_rule(pattern=["pip"], decision="prompt")
prefix_rule(pattern=["pip3"], decision="prompt")
prefix_rule(pattern=["move"], decision="prompt")
prefix_rule(pattern=["ren"], decision="prompt")
prefix_rule(pattern=["rename"], decision="prompt")
prefix_rule(pattern=["mv"], decision="prompt")
prefix_rule(pattern=["git", "reset"], decision="prompt")
prefix_rule(pattern=["git", "clean"], decision="prompt")
"#;
        std::fs::write(codex_home.join("rules").join("office.policy"), policy)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 启动 app-server 子进程（stdio 传输）。
    pub async fn spawn(
        bundled: &Bundled,
        codex_home: &Path,
        settings: &AppSettings,
        api_key: &str,
    ) -> Result<Self, CodexError> {
        let app_server = bundled.codex_exe();
        if !app_server.exists() {
            return Err(CodexError::Other(format!(
                "未找到 codex app-server: {}",
                app_server.display()
            )));
        }
        // 若使用主 codex.exe，则需追加 app-server 子命令
        let is_main_binary = app_server
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase().starts_with("codex."))
            .unwrap_or(false);

        // 环境变量
        let python_dir = bundled.python_dir();
        let codex_dir = bundled.codex_dir();
        let lo_program = bundled.libreoffice_dir().join("program");
        let mut dirs = vec![python_dir.clone(), codex_dir];
        if lo_program.exists() {
            dirs.push(lo_program); // LibreOffice 供 agent 直接调用 soffice
        }
        let path = std::env::join_paths(
            dirs.into_iter()
                .chain(std::env::split_paths(&std::env::var("PATH").unwrap_or_default())),
        )
        .unwrap_or_default();
        let path_str = path.to_string_lossy().to_string();

        let mut cmd = Command::new(&app_server);
        if is_main_binary {
            cmd.arg("app-server");
        }
        // Windows 下抑制子进程控制台窗口（避免启动引擎时弹出黑窗口）
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.env("CODEX_HOME", codex_home)
            .env(settings.api_key_env.clone(), api_key)
            .env("PATH", path_str)
            .env("RUST_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(codex_home);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| CodexError::Other("无法获取 stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| CodexError::Other("无法获取 stdout".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| CodexError::Other("无法获取 stderr".into()))?;

        let (events_tx, events_rx) = mpsc::unbounded_channel::<EngineEvent>();
        let (out_tx, out_rx) = mpsc::unbounded_channel::<String>();

        let next_id = Arc::new(AtomicI64::new(1));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let server_requests = Arc::new(Mutex::new(HashMap::new()));
        let thread_id = Arc::new(Mutex::new(None));
        let turn_id = Arc::new(Mutex::new(None));
        let running = Arc::new(AtomicBool::new(true));

        // writer task
        let mut wstdin = stdin;
        let wrunning = running.clone();
        tokio::spawn(async move {
            let mut rx = out_rx;
            while let Some(line) = rx.recv().await {
                if wstdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if wstdin.write_all(b"\n").await.is_err() {
                    break;
                }
                if wstdin.flush().await.is_err() {
                    break;
                }
            }
            let _ = wrunning;
        });

        // reader task
        let rnext_id = next_id.clone();
        let rpending = pending.clone();
        let rserver_requests = server_requests.clone();
        let rthread_id = thread_id.clone();
        let rturn_id = turn_id.clone();
        let rrunning = running.clone();
        let retx = events_tx.clone();
        let out2 = out_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                Self::dispatch_line(
                    &line,
                    &retx,
                    &out2,
                    &rnext_id,
                    &rpending,
                    &rserver_requests,
                    &rthread_id,
                    &rturn_id,
                )
                .await;
            }
            // stdout 关闭
            let _ = retx.send(EngineEvent::EngineStopped);
            rrunning.store(false, Ordering::SeqCst);
        });

        // stderr task → log（同时写入日志文件）
        let ltx = events_tx.clone();
        let log_dir = settings.log_dir.clone();
        tokio::spawn(async move {
            // 同时把 stderr 写入日志文件（若配置了日志目录）
            let mut log_file = if log_dir.trim().is_empty() {
                None
            } else {
                let dir = std::path::Path::new(&log_dir);
                if std::fs::create_dir_all(dir).is_ok() {
                    tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(dir.join("harness.log"))
                        .await
                        .ok()
                } else {
                    None
                }
            };
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = ltx.send(EngineEvent::Log { level: "stderr".into(), msg: line.clone() });
                if let Some(f) = log_file.as_mut() {
                    use tokio::io::AsyncWriteExt;
                    let _ = f.write_all(format!("{}\n", line).as_bytes()).await;
                }
            }
        });

        Ok(Self {
            child: Some(child),
            out_tx,
            events: Some(events_rx),
            next_id,
            pending,
            server_requests,
            thread_id,
            turn_id,
            running,
        })
    }

    async fn dispatch_line(
        line: &str,
        events: &mpsc::UnboundedSender<EngineEvent>,
        out: &mpsc::UnboundedSender<String>,
        next_id: &Arc<AtomicI64>,
        pending: &PendingMap,
        server_requests: &Arc<Mutex<HashMap<i64, String>>>,
        thread_id: &Arc<Mutex<Option<String>>>,
        turn_id: &Arc<Mutex<Option<String>>>,
    ) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            let _ = events.send(EngineEvent::Log { level: "raw".into(), msg: line.to_string() });
            return;
        };
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let has_id = v.get("id").is_some();

        if has_id {
            let id = v.get("id").and_then(|i| i.as_i64()).unwrap_or(-1);
            if v.get("result").is_some() || v.get("error").is_some() {
                // 对我们请求的响应
                if let Some(tx) = pending.lock().await.remove(&id) {
                    let res = if let Some(err) = v.get("error") {
                        Err(CodexError::Rpc(err.to_string()))
                    } else {
                        Ok(v.get("result").cloned().unwrap_or(Value::Null))
                    };
                    let _ = tx.send(res);
                }
                return;
            }
            if !method.is_empty() {
                // 服务端发起的请求（审批等）
                handle_server_request(id, &method, &v, events, out, server_requests).await;
                return;
            }
        }

        // 通知
        handle_notification(&method, &v, events, thread_id, turn_id).await;
        let _ = next_id;
    }

    /// 发送一个请求并等待响应。
    async fn request(&self, method: &str, params: Value) -> Result<Value, CodexError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(CodexError::NotRunning);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if self.out_tx.send(msg.to_string()).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(CodexError::NotRunning);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(CodexError::Other("通道关闭".into())),
            Err(_) => Err(CodexError::Timeout),
        }
    }

    pub fn events(&mut self) -> &mut mpsc::UnboundedReceiver<EngineEvent> {
        self.events
            .as_mut()
            .expect("events receiver already taken")
    }

    /// 取出事件接收端（由事件转发任务持有）。
    pub fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<EngineEvent>> {
        self.events.take()
    }

    /// 初始化握手。
    pub async fn initialize(&mut self) -> Result<(), CodexError> {
        let params = json!({
            "clientInfo": { "name": "office_harness", "title": "办公自动化助手", "version": "0.1.0" },
            "capabilities": { "experimentalApi": true }
        });
        self.request("initialize", params).await.map(|_| ())?;
        // 发送 initialized 通知
        let _ = self.out_tx.send(json!({ "jsonrpc": "2.0", "method": "initialized" }).to_string());
        Ok(())
    }

    /// 启动会话（线程）。
    pub async fn start_thread(
        &mut self,
        workspace: &str,
        sandbox: &str,
        model: &str,
        skills_repo: &str,
    ) -> Result<String, CodexError> {
        // 沙箱枚举在线上为 kebab-case：read-only | workspace-write | danger-full-access
        let sandbox = match sandbox {
            "read-only" => "read-only",
            "danger-full-access" => "danger-full-access",
            _ => "workspace-write",
        };
        // 开发者指令：基础方法论 + 技能系统说明（含仓库路径）
        let developer_instructions = format!(
            "{}\n\n## 技能系统\n- 你拥有一个 SKILLS 技能仓库，位于：`{}`。\n- 仓库下每个子目录是一个技能，内含 SKILL.md（YAML frontmatter 声明 name 与 description）。\n- 处理用户任务时：先判断用户意图是否匹配某个已有技能；匹配则优先调用该技能（用 $技能名 方式触发）来完成任务。\n- 用户可以用自然语言让你创建、修改、删除技能：\n  1. 创建：新建目录 + 编写 SKILL.md（name/description）+ 必要时附带可执行的 Python/脚本步骤；\n  2. 修改：编辑对应 SKILL.md；\n  3. 删除：删除对应技能目录。\n- 技能文件必须放在技能仓库目录内，遵循仓库内已有技能的格式。\n- 维护技能属于对文件的修改操作，同样遵守工作区审批规则。",
            crate::prompts::DEVELOPER_INSTRUCTIONS,
            skills_repo,
        );
        let params = json!({
            "model": model,
            "cwd": workspace,
            "sandbox": sandbox,
            "approvalPolicy": "on-request",
            "personality": "pragmatic",
            "developerInstructions": developer_instructions,
        });
        let res = self.request("thread/start", params).await?;
        let tid = res
            .pointer("/thread/id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CodexError::Other("thread/start 响应缺少 thread.id".into()))?
            .to_string();
        *self.thread_id.lock().await = Some(tid.clone());
        Ok(tid)
    }

    /// 发送用户消息，开始一轮。
    pub async fn send_turn(&mut self, text: &str) -> Result<String, CodexError> {
        let tid = self
            .thread_id
            .lock()
            .await
            .clone()
            .ok_or_else(|| CodexError::Other("会话未启动".into()))?;
        let params = json!({
            "threadId": tid,
            "input": [ { "type": "text", "text": text } ],
        });
        let res = self.request("turn/start", params).await?;
        let turn = res
            .pointer("/turn/id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| CodexError::Other("turn/start 响应缺少 turn.id".into()))?;
        *self.turn_id.lock().await = Some(turn.clone());
        Ok(turn)
    }

    /// 列出指定工作区下的会话（线程）。
    pub async fn list_threads(&mut self, cwd: &str) -> Result<Vec<ThreadInfo>, CodexError> {
        let params = json!({
            "cwd": cwd,
            "limit": 100,
            "sortKey": "recency_at",
        });
        let res = self.request("thread/list", params).await?;
        let mut out = Vec::new();
        if let Some(arr) = res.get("data").and_then(|d| d.as_array()) {
            for t in arr {
                let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if id.is_empty() {
                    continue;
                }
                out.push(ThreadInfo {
                    id,
                    preview: t.get("preview").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    created_at: t.get("createdAt").and_then(|v| v.as_i64()).unwrap_or(0),
                    updated_at: t.get("updatedAt").and_then(|v| v.as_i64()).unwrap_or(0),
                });
            }
        }
        Ok(out)
    }

    /// 恢复（切换）到指定会话。
    pub async fn resume_thread(&mut self, thread_id: &str) -> Result<String, CodexError> {
        let params = json!({ "threadId": thread_id });
        let res = self.request("thread/resume", params).await?;
        let tid = res
            .pointer("/thread/id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| CodexError::Other("thread/resume 响应缺少 thread.id".into()))?;
        *self.thread_id.lock().await = Some(tid.clone());
        *self.turn_id.lock().await = None;
        Ok(tid)
    }

    /// 删除指定会话。
    pub async fn delete_thread(&mut self, thread_id: &str) -> Result<(), CodexError> {
        let params = json!({ "threadId": thread_id });
        self.request("thread/delete", params).await.map(|_| ())
    }

    /// 读取会话历史（含 turns），返回用户/助手消息列表。
    pub async fn read_thread_history(&mut self, thread_id: &str) -> Result<Vec<HistoryMessage>, CodexError> {
        let params = json!({ "threadId": thread_id, "includeTurns": true });
        let res = self.request("thread/read", params).await?;
        let mut out = Vec::new();
        if let Some(turns) = res.pointer("/thread/turns").and_then(|t| t.as_array()) {
            for turn in turns {
                if let Some(items) = turn.get("items").and_then(|i| i.as_array()) {
                    for item in items {
                        let itype = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if text.trim().is_empty() {
                            continue;
                        }
                        match itype {
                            "userMessage" => out.push(HistoryMessage { role: "user".into(), text }),
                            "agentMessage" => out.push(HistoryMessage { role: "assistant".into(), text }),
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// 注册额外的 SKILLS 仓库根（codex skills/extraRoots/set）。
    pub async fn register_skills_roots(&mut self, roots: &[String]) -> Result<(), CodexError> {
        let params = json!({ "extraRoots": roots });
        self.request("skills/extraRoots/set", params).await.map(|_| ())
    }

    /// 列出引擎视角的技能（含额外仓库）。
    pub async fn list_skills(&mut self, cwd: &str) -> Result<Value, CodexError> {
        let params = json!({ "cwds": [cwd], "forceReload": true });
        self.request("skills/list", params).await
    }

    /// 中断当前轮。
    pub async fn interrupt(&mut self) -> Result<(), CodexError> {
        let (tid, turn) = {
            let t = self.thread_id.lock().await;
            let n = self.turn_id.lock().await;
            (t.clone(), n.clone())
        };
        let tid = tid.ok_or_else(|| CodexError::Other("会话未启动".into()))?;
        let turn = turn.ok_or_else(|| CodexError::Other("当前没有进行中的轮次".into()))?;
        let params = json!({ "threadId": tid, "turnId": turn });
        self.request("turn/interrupt", params).await.map(|_| ())
    }

    /// 响应服务端审批请求。
    pub async fn respond_approval(&mut self, request_id: i64, decision: &str) -> Result<(), CodexError> {
        let msg = json!({ "jsonrpc": "2.0", "id": request_id, "result": { "decision": decision } });
        self.out_tx
            .send(msg.to_string())
            .map_err(|_| CodexError::NotRunning)
    }

    /// 触发 Windows 沙箱配置。mode: "elevated"（提权，弹 UAC）或 "unelevated"（免授权）。
    pub async fn setup_windows_sandbox_mode(
        &mut self,
        workspace: &str,
        mode: &str,
    ) -> Result<(), CodexError> {
        let mode = if mode == "elevated" { "elevated" } else { "unelevated" };
        let params = json!({ "mode": mode, "cwd": workspace });
        self.request("windowsSandbox/setupStart", params).await.map(|_| ())
    }

    /// 查询 Windows 沙箱就绪状态（Ready / NotConfigured / UpdateRequired）。
    pub async fn sandbox_readiness(&mut self) -> Result<String, CodexError> {
        let res = self.request("windowsSandbox/readiness", json!({})).await?;
        Ok(res
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    /// 自动拒绝所有未处理的服务端请求（用于停止/清理时）。
    pub async fn auto_decline_all(&mut self) {
        let reqs: Vec<i64> = self.server_requests.lock().await.keys().cloned().collect();
        for id in reqs {
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "decision": "decline" } });
            let _ = self.out_tx.send(msg.to_string());
        }
    }

    /// 停止引擎。
    pub async fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.auto_decline_all().await;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    /// 同步强制终止子进程（用于应用退出时，避免孤儿进程）。
    pub fn kill_now(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }

    /// 子进程 PID。
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.id())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 当前会话 id。
    pub async fn current_thread_id(&self) -> Option<String> {
        self.thread_id.lock().await.clone()
    }
}

async fn handle_server_request(
    id: i64,
    method: &str,
    v: &Value,
    events: &mpsc::UnboundedSender<EngineEvent>,
    out: &mpsc::UnboundedSender<String>,
    server_requests: &Arc<Mutex<HashMap<i64, String>>>,
) {
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    server_requests.lock().await.insert(id, method.to_string());
    match method {
        "item/commandExecution/requestApproval" => {
            let command = params.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let cwd = params.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let reason = params.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
            let item_id = params.get("itemId").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::ApprovalRequest {
                request_id: id,
                kind: "command".into(),
                item_id,
                command,
                cwd,
                reason,
                changes: String::new(),
            });
        }
        "item/fileChange/requestApproval" => {
            let item_id = params.get("itemId").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let reason = params.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
            let changes = params
                .get("changes")
                .map(|c| serde_json::to_string(c).unwrap_or_default())
                .unwrap_or_default();
            let _ = events.send(EngineEvent::ApprovalRequest {
                request_id: id,
                kind: "fileChange".into(),
                item_id,
                command: String::new(),
                cwd: String::new(),
                reason,
                changes,
            });
        }
        "item/tool/requestUserInput" => {
            // 不支持，自动拒绝
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "value": null } });
            let _ = out.send(msg.to_string());
        }
        "mcpServer/elicitation/request" => {
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "action": "decline", "content": null } });
            let _ = out.send(msg.to_string());
        }
        "item/tool/call" => {
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "contentItems": [], "success": false } });
            let _ = out.send(msg.to_string());
        }
        "currentTime/read" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "currentTimeAt": now } });
            let _ = out.send(msg.to_string());
        }
        _ => {
            // 未知请求：失败关闭
            let msg = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "unsupported by office harness" } });
            let _ = out.send(msg.to_string());
        }
    }
}

async fn handle_notification(
    method: &str,
    v: &Value,
    events: &mpsc::UnboundedSender<EngineEvent>,
    thread_id: &Arc<Mutex<Option<String>>>,
    turn_id: &Arc<Mutex<Option<String>>>,
) {
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "thread/started" => {
            if let Some(tid) = params.pointer("/thread/id").and_then(|i| i.as_str()) {
                *thread_id.lock().await = Some(tid.to_string());
                let _ = events.send(EngineEvent::ThreadStarted { thread_id: tid.to_string() });
            }
        }
        "turn/started" => {
            let turn = params.get("turn").cloned().unwrap_or(Value::Null);
            let turn_id_str = turn.get("id").and_then(|t| t.as_str()).unwrap_or("").to_string();
            if !turn_id_str.is_empty() {
                *turn_id.lock().await = Some(turn_id_str.clone());
            }
            let _ = events.send(EngineEvent::TurnStarted { turn_id: turn_id_str });
        }
        "item/agentMessage/delta" => {
            let text = params.get("delta").and_then(|d| d.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::AgentDelta { text });
        }
        "item/agentMessage/completed" => {
            let text = params.pointer("/item/text").and_then(|d| d.as_str()).unwrap_or("").to_string();
            if !text.is_empty() {
                let _ = events.send(EngineEvent::AgentMessage { text });
            }
        }
        "item/reasoning/summaryTextDelta" => {
            // 思考过程：只转发摘要（简洁），完整思考细节不推送，避免界面噪音
            let text = params.get("delta").and_then(|d| d.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::ReasoningDelta { text });
        }
        "item/reasoning/textDelta" => {
            // 详细思考过程：丢弃，不展示给用户
        }
        "item/started" => {
            let item = params.get("item").cloned().unwrap_or(Value::Null);
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            match item_type.as_str() {
                "commandExecution" => {
                    let command = item.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    let cwd = item.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    // 应用层破坏性命令扫描（对服务端未要求审批的命令补充透明标记）
                    let dangerous: Vec<String> = crate::scanner::classify_command(&command)
                        .into_iter()
                        .map(|m| m.label)
                        .collect();
                    let _ = events.send(EngineEvent::CommandStarted { item_id, command, cwd, dangerous });
                }
                "fileChange" => {
                    let summary = item
                        .get("changes")
                        .map(|c| serde_json::to_string(c).unwrap_or_default())
                        .unwrap_or_default();
                    let _ = events.send(EngineEvent::FileChangeStarted { item_id, summary });
                }
                _ => {}
            }
        }
        "item/completed" => {
            let item = params.get("item").cloned().unwrap_or(Value::Null);
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            match item_type.as_str() {
                "commandExecution" => {
                    let command = item.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let output = item
                        .get("aggregatedOutput")
                        .and_then(|o| o.as_str())
                        .unwrap_or("")
                        .to_string();
                    let _ = events.send(EngineEvent::CommandCompleted { item_id, command, status, output });
                }
                "fileChange" => {
                    let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let _ = events.send(EngineEvent::FileChangeCompleted { item_id, status });
                }
                "agentMessage" => {
                    let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    if !text.is_empty() {
                        let _ = events.send(EngineEvent::AgentMessage { text });
                    }
                }
                _ => {}
            }
        }
        "item/commandExecution/outputDelta" => {
            let item_id = params.get("itemId").and_then(|i| i.as_str()).unwrap_or("").to_string();
            let output = params.get("delta").and_then(|d| d.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::CommandOutput { item_id, output });
        }
        "serverRequest/resolved" => {
            let rid = params.get("requestId").and_then(|r| r.as_i64()).unwrap_or(-1);
            let _ = events.send(EngineEvent::ApprovalResolved { request_id: rid });
        }
        "windowsSandbox/setupCompleted" => {
            let success = params.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
            let mode = params.get("mode").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let error = params.get("error").and_then(|e| e.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::SandboxSetupResult { success, mode, error });
        }
        "turn/completed" => {
            let turn = params.get("turn").cloned().unwrap_or(Value::Null);
            let status = turn.get("status").and_then(|s| s.as_str()).unwrap_or("completed").to_string();
            let usage = turn
                .get("usage")
                .map(|u| serde_json::to_string(u).unwrap_or_default())
                .unwrap_or_default();
            let _ = events.send(EngineEvent::TurnCompleted { status, usage });
        }
        "error" => {
            let msg = params.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::Log { level: "error".into(), msg });
        }
        "warning" => {
            let msg = params.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let _ = events.send(EngineEvent::Log { level: "warning".into(), msg });
        }
        _ => {
            let _ = events.send(EngineEvent::Unknown {
                method: method.to_string(),
                payload: v.to_string(),
            });
        }
    }
}
