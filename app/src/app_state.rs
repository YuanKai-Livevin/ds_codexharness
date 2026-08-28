//! 应用全局状态与数据根目录。

use oh_core::codex::CodexServer;
use oh_core::config::AppSettings;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

/// 记忆面板/翻译层服务端口（与 oh-core 常量保持一致）。
pub(crate) const MEMORY_PORT: u16 = oh_core::MEMORY_PORT;

/// 统一引擎状态机（T0-06）：由本状态为唯一真相源，前端只消费这一个状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum EngineState {
    Stopped,
    Starting,
    Ready,
    Busy,
    Stopping,
    Failed,
}

impl EngineState {
    pub(crate) fn is_running(self) -> bool {
        matches!(self, EngineState::Ready | EngineState::Busy)
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            EngineState::Stopped => "stopped",
            EngineState::Starting => "starting",
            EngineState::Ready => "ready",
            EngineState::Busy => "busy",
            EngineState::Stopping => "stopping",
            EngineState::Failed => "failed",
        }
    }
}

pub(crate) struct AppState {
    pub(crate) settings_path: PathBuf,
    pub(crate) codex_home: PathBuf,
    pub(crate) settings: Mutex<AppSettings>,
    pub(crate) api_key: Mutex<Option<String>>,
    pub(crate) engine: Mutex<Option<CodexServer>>,
    pub(crate) engine_pid: Mutex<Option<u32>>,
    pub(crate) engine_running: AtomicBool,
    pub(crate) engine_state: Mutex<EngineState>,
    // 本地 sidecar（R2）：模型网关 + 记忆服务，独立进程/随机端口/会话令牌
    pub(crate) gateway_pid: Mutex<Option<u32>>,
    pub(crate) gateway_port: Mutex<Option<u16>>,
    pub(crate) memory_pid: Mutex<Option<u32>>,
    pub(crate) memory_port: Mutex<Option<u16>>,
    pub(crate) session_token: Mutex<Option<String>>,
}

/// 应用数据根目录：所有应用级配置/数据集中在一个目录便于管理。
/// 包含 settings.json、codex-home（引擎）、skills（技能库）、logs（日志）。
pub(crate) fn data_root() -> PathBuf {
    PathBuf::from("C:\\HARNESS")
}

/// 迁移旧版数据目录（%APPDATA%\OfficeHarness）到新根目录 C:\HARNESS。
/// 只迁移尚未存在的新位置，避免覆盖用户新数据；旧目录残留由用户自行清理。
pub(crate) fn migrate_legacy_data_root() {
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
