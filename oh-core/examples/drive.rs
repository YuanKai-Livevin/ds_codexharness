//! 端到端驱动测试：启动真实 app-server → initialize → thread/start → turn/start → 事件流。
//! 运行: cargo run --example drive (在项目根目录，需 OH_DEV_ROOT 或位于项目根运行)
use oh_core::codex::CodexServer;
use oh_core::config::AppSettings;
use oh_core::model::EngineEvent;
use oh_core::python::Bundled;
use std::path::Path;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    std::env::set_var("OH_DEV_ROOT", project);

    // 临时 CODEX_HOME 与工作区
    let home = std::env::temp_dir().join(format!("oh-drive-{}", std::process::id()));
    let ws = std::env::temp_dir().join(format!("oh-drive-ws-{}", std::process::id()));
    std::fs::create_dir_all(&home).ok();
    std::fs::create_dir_all(&ws).ok();

    let settings = AppSettings {
        workspace_path: ws.to_string_lossy().to_string(),
        provider_name: "deepseek".into(),
        base_url: "https://api.deepseek.com/".into(),
        model: "deepseek-v4-flash".into(),
        api_key_env: "OH_DRIVE_KEY".into(),
        sandbox_mode: "workspace-write".into(),
        windows_sandbox: "unelevated".into(),
        onboarded: true,
        api_key_enc: None,
        recent_workspaces: Vec::new(),
        log_dir: "".into(),
        no_auth: false,
        use_bridge: false,
        show_commands: false,
    };

    println!("== prepare_home ==");
    CodexServer::prepare_home(&home, &settings, None)?;

    let bundled = Bundled::new(None);
    println!("codex: {}", bundled.codex_exe().display());
    println!("python: {}", bundled.python_exe().display());

    println!("== spawn ==");
    let mut server = CodexServer::spawn(&bundled, &home, &settings, "sk-fake-key")
        .await
        .map_err(|e| format!("spawn: {}", e))?;

    println!("== initialize ==");
    server
        .initialize()
        .await
        .map_err(|e| format!("init: {}", e))?;

    println!("== thread/start ==");
    let tid = server
        .start_thread(
            &settings.workspace_path,
            "workspace-write",
            "deepseek-v4-flash",
            "",
        )
        .await
        .map_err(|e| format!("thread: {}", e))?;
    println!("thread id: {}", tid);

    println!("== turn/start ==");
    let turn = server
        .send_turn("你好，请确认你能访问工作区并输出【执行计划】。")
        .await
        .map_err(|e| format!("turn: {}", e))?;
    println!("turn id: {}", turn);

    // 等待事件流
    let mut events = server.take_events().ok_or("no events")?;
    let mut saw_completed = false;
    for _ in 0..600 {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if saw_completed { break; }
            }
            ev = events.recv() => {
                match ev {
                    Some(EngineEvent::AgentDelta { text }) => print!("{}", text),
                    Some(EngineEvent::AgentMessage { text }) => println!("\n[final] {}", text),
                    Some(EngineEvent::ApprovalRequest { command, reason, .. }) => {
                        println!("\n[approval] {} | {}", command, reason);
                        // 测试自动允许
                        // server.respond_approval(request_id, "accept").await?;
                    }
                    Some(EngineEvent::CommandStarted { command, .. }) => println!("\n[cmd] {}", command),
                    Some(EngineEvent::CommandCompleted { status, output, .. }) => {
                        println!("\n[cmd done] {} | {}", status, output.chars().take(200).collect::<String>());
                    }
                    Some(EngineEvent::TurnCompleted { status, .. }) => {
                        println!("\n[turn completed] status={}", status);
                        saw_completed = true;
                    }
                    Some(EngineEvent::Log { level, msg }) => {
                        if level == "error" { println!("\n[log:{}] {}", level, msg); }
                    }
                    Some(EngineEvent::EngineStopped) => { println!("\n[engine stopped]"); break; }
                    Some(_) => {}
                    None => break,
                }
            }
        }
    }

    println!("== stop ==");
    server.stop().await;
    println!("DONE");
    Ok(())
}
