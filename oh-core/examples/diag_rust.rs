//! Rust 驱动全链路诊断：真实 key + 真实工作区，打印收到的所有事件。
//! 运行: cargo run --example diag_rust -p oh-core
use oh_core::codex::CodexServer;
use oh_core::config::AppSettings;
use oh_core::dpapi;
use oh_core::model::EngineEvent;
use oh_core::python::Bundled;
use std::path::Path;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    std::env::set_var("OH_DEV_ROOT", project);

    // 读取真实设置
    let settings_path = std::env::var("APPDATA").unwrap_or_default();
    let settings_path = std::path::Path::new(&settings_path)
        .join("OfficeHarness")
        .join("settings.json");
    let settings = AppSettings::load(&settings_path);
    let key = settings
        .api_key_enc
        .as_deref()
        .map(|e| dpapi::decrypt(e))
        .transpose()?
        .unwrap_or_default();
    println!("workspace: {}", settings.workspace_path);
    println!("model: {}", settings.model);

    let home = std::env::temp_dir().join(format!("oh-diag-{}", std::process::id()));
    std::fs::create_dir_all(&home).ok();
    CodexServer::prepare_home(&home, &settings, None)?;

    let bundled = Bundled::new(None);
    let mut server = CodexServer::spawn(&bundled, &home, &settings, &key).await?;
    server.initialize().await?;
    let tid = server
        .start_thread(
            &settings.workspace_path,
            "workspace-write",
            &settings.model,
            "",
        )
        .await?;
    println!("thread: {}", tid);

    let turn = server.send_turn("你好，请只回复四个字：连接成功。").await?;
    println!("turn: {}", turn);

    let mut events = server.take_events().ok_or("no events")?;
    let mut got_reply = false;
    for _ in 0..600 {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if got_reply { break; }
            }
            ev = events.recv() => match ev {
                Some(EngineEvent::AgentDelta { text }) => {
                    print!("{}", text);
                    got_reply = true;
                }
                Some(EngineEvent::AgentMessage { text }) => {
                    println!("\n[final] {}", text);
                    got_reply = true;
                }
                Some(EngineEvent::TurnCompleted { status, .. }) => {
                    println!("\n[turn completed] {}", status);
                    got_reply = true;
                }
                Some(EngineEvent::Log { level, msg }) => {
                    if level == "error" || level == "warning" {
                        println!("\n[{}] {}", level, msg);
                    }
                }
                Some(EngineEvent::ApprovalRequest { command, .. }) => {
                    println!("\n[approval needed] {}", command);
                }
                Some(EngineEvent::EngineStopped) => { println!("\n[engine stopped]"); break; }
                Some(_) => {}
                None => break,
            }
        }
    }
    if !got_reply {
        println!("\n!!! Rust 驱动未收到任何回复事件 !!!");
    }
    server.stop().await;
    println!("\nDONE");
    Ok(())
}
