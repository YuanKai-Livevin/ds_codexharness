//! 办公自动化助手 —— Tauri 应用主体（库形式，供 main 调用）。
//! 命令与逻辑按模块拆分：app_state / commands/* / services/*。

mod app_state;
mod commands;
mod services;

use app_state::{migrate_legacy_data_root, AppState};
use commands::{
    engine::{start_engine, start_engine_inner, stop_engine, send_message, respond_approval, interrupt, setup_sandbox, sandbox_status},
    memory::{memory_status, open_memory_panel},
    office::{libreoffice_status, open_in_libreoffice, convert_office},
    sessions::{list_sessions, current_session, new_session, switch_session, delete_session, session_history, list_tmp, cleanup_tmp},
    settings::{get_settings, save_settings, save_api_key, has_api_key, get_api_key_masked, test_connection, get_status},
    skills::{get_skills_repo, open_skills_repo, import_skills},
    workspace::{remove_workspace, common_folders, open_workspace, pick_folder, list_dir, list_workspace_files, open_path},
};
use oh_core::config::AppSettings;
use oh_core::model::EngineEvent;
use std::sync::atomic::AtomicBool;
use tauri::{Emitter, Manager, RunEvent};
use tokio::sync::Mutex;

pub fn run() {
    // 旧数据目录（%APPDATA%\OfficeHarness）自动迁移到 C:\HARNESS
    migrate_legacy_data_root();
    let root = app_state::data_root();
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
            // 启动时自动拉起引擎（若已配置 API Key 或内网免密钥）
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
