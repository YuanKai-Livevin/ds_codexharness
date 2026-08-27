//! 应用全局状态与数据根目录。

use oh_core::codex::CodexServer;
use oh_core::config::AppSettings;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

/// 记忆面板/翻译层服务端口（与 oh-core 常量保持一致）。
pub(crate) const MEMORY_PORT: u16 = oh_core::MEMORY_PORT;

pub(crate) struct AppState {
    pub(crate) settings_path: PathBuf,
    pub(crate) codex_home: PathBuf,
    pub(crate) settings: Mutex<AppSettings>,
    pub(crate) api_key: Mutex<Option<String>>,
    pub(crate) engine: Mutex<Option<CodexServer>>,
    pub(crate) engine_pid: Mutex<Option<u32>>,
    pub(crate) engine_running: AtomicBool,
    pub(crate) memory_pid: Mutex<Option<u32>>,
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
