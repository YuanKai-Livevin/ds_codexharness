//! oh-core：办公自动化助手核心逻辑库。

/// 记忆面板/翻译层服务端口（uvicorn 固定监听）。
/// 引擎在启用内置翻译层时，base_url 指向 http://127.0.0.1:{MEMORY_PORT}/
pub const MEMORY_PORT: u16 = 8765;

pub mod codex;
pub mod config;
pub mod dpapi;
pub mod model;
pub mod prompts;
pub mod python;
pub mod scanner;
pub mod winproc;
pub mod workspace;
