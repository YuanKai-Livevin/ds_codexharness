//! 引擎事件模型（codex app-server → UI）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EngineEvent {
    /// 引擎生命周期状态。
    Status { state: String, detail: String },
    /// 日志（中文/调试信息）。
    Log { level: String, msg: String },
    /// 会话已建立。
    ThreadStarted { thread_id: String },
    /// 新一轮开始。
    TurnStarted { turn_id: String },
    /// 助手消息流式增量。
    AgentDelta { text: String },
    /// 助手最终消息。
    AgentMessage { text: String },
    /// 思考过程增量（可折叠展示）。
    ReasoningDelta { text: String },
    /// 命令项开始。
    CommandStarted {
        item_id: String,
        command: String,
        cwd: String,
        /// 应用层扫描命中的破坏性模式标签（如 "删除文件/目录 (rm)"）；空表示未命中。
        dangerous: Vec<String>,
    },
    /// 命令输出增量。
    CommandOutput { item_id: String, output: String },
    /// 命令项结束。
    CommandCompleted {
        item_id: String,
        command: String,
        status: String,
        output: String,
    },
    /// 文件变更项开始（含摘要）。
    FileChangeStarted { item_id: String, summary: String },
    /// 文件变更项结束。
    FileChangeCompleted { item_id: String, status: String },
    /// 需要用户审批（危险操作）。
    ApprovalRequest {
        request_id: i64,
        kind: String,
        item_id: String,
        command: String,
        cwd: String,
        reason: String,
        changes: String,
    },
    /// 审批请求已被服务端关闭（无需再等待）。
    ApprovalResolved { request_id: i64 },
    /// Windows 沙箱配置完成（真实结果）。
    SandboxSetupResult {
        success: bool,
        mode: String,
        error: String,
    },
    /// 本轮结束。
    TurnCompleted { status: String, usage: String },
    /// 引擎已停止。
    EngineStopped,
    /// 未识别事件（调试用）。
    Unknown { method: String, payload: String },
}

impl EngineEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}
