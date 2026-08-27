//! codex app-server 驱动子模块：客户端 / 事件分发 / 审批请求。

pub mod approvals;
pub mod client;
pub mod events;

pub use client::{CodexError, CodexServer, HistoryMessage, ThreadInfo};
