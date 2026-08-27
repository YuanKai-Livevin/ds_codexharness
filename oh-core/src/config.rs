//! Application settings persistence.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    /// 工作区路径 —— 所有文件操作的唯一允许目录。
    pub workspace_path: String,
    /// API 提供商名称（用于模型提供商配置）。
    pub provider_name: String,
    /// OpenAI 兼容 API 基地址。
    pub base_url: String,
    /// 模型名。
    pub model: String,
    /// API Key 环境变量名（值不落盘，由用户输入后写入进程环境）。
    pub api_key_env: String,
    /// 沙箱模式: workspace-write | default | none
    pub sandbox_mode: String,
    /// Windows 沙箱实现: unelevated（受限令牌，免配置，推荐）| elevated（提权，需 UAC 授权）
    pub windows_sandbox: String,
    /// 是否已同意提示（首次使用展示说明）。
    pub onboarded: bool,
    /// API Key 的 DPAPI 密文（base64），仅本机本用户可解密；不会明文落盘。
    pub api_key_enc: Option<String>,
    /// 最近使用过的工作区（用于快速切换，最多保留 6 个）。
    pub recent_workspaces: Vec<String>,
    /// 引擎日志目录（默认 C:\HARNESS\logs）。
    pub log_dir: String,
    /// 内网免密钥模式：true 时不需要 API Key（provider 配置 requires_openai_auth=false）。
    pub no_auth: bool,
    /// 使用内置翻译层：true 时引擎走本地 /responses→/chat/completions 翻译器，
    /// 适用于内网网关仅支持 chat/completions 的场景。
    pub use_bridge: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        let default_workspace = dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("办公工作区")
            .to_string_lossy()
            .to_string();
        Self {
            workspace_path: default_workspace,
            provider_name: "deepseek".to_string(),
            base_url: "https://api.deepseek.com/".to_string(),
            model: "deepseek-v4-flash".to_string(),
            api_key_env: "OH_API_KEY".to_string(),
            sandbox_mode: "workspace-write".to_string(),
            windows_sandbox: "unelevated".to_string(),
            onboarded: false,
            api_key_enc: None,
            recent_workspaces: Vec::new(),
            log_dir: "C:\\HARNESS\\logs".to_string(),
            no_auth: false,
            use_bridge: false,
        }
    }
}

impl AppSettings {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}
