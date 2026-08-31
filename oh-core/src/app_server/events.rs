//! app-server 事件（通知）分发：把服务端推送的 JSON-RPC 通知转成 EngineEvent。

use crate::model::EngineEvent;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// 处理服务端推送的通知（无 id 的消息）。
pub(crate) async fn handle_notification(
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
                let _ = events.send(EngineEvent::ThreadStarted {
                    thread_id: tid.to_string(),
                });
            }
        }
        "turn/started" => {
            let turn = params.get("turn").cloned().unwrap_or(Value::Null);
            let turn_id_str = turn
                .get("id")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            if !turn_id_str.is_empty() {
                *turn_id.lock().await = Some(turn_id_str.clone());
            }
            let _ = events.send(EngineEvent::TurnStarted {
                turn_id: turn_id_str,
            });
        }
        "item/agentMessage/delta" => {
            let text = params
                .get("delta")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::AgentDelta { text });
        }
        "item/agentMessage/completed" => {
            let text = params
                .pointer("/item/text")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            if !text.is_empty() {
                let _ = events.send(EngineEvent::AgentMessage { text });
            }
        }
        "item/reasoning/summaryTextDelta" => {
            // 思考过程：只转发摘要（简洁），完整思考细节不推送，避免界面噪音
            let text = params
                .get("delta")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::ReasoningDelta { text });
        }
        "item/reasoning/textDelta" => {
            // 详细思考过程：丢弃，不展示给用户
        }
        "item/started" => {
            let item = params.get("item").cloned().unwrap_or(Value::Null);
            let item_type = item
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let item_id = item
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            match item_type.as_str() {
                "commandExecution" => {
                    let command = item
                        .get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let cwd = item
                        .get("cwd")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    // 应用层破坏性命令扫描（对服务端未要求审批的命令补充透明标记）
                    let dangerous: Vec<String> = crate::scanner::classify_command(&command)
                        .into_iter()
                        .map(|m| m.label)
                        .collect();
                    let _ = events.send(EngineEvent::CommandStarted {
                        item_id,
                        command,
                        cwd,
                        dangerous,
                    });
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
            let item_type = item
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let item_id = item
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            match item_type.as_str() {
                "commandExecution" => {
                    let command = item
                        .get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let status = item
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let output = item
                        .get("aggregatedOutput")
                        .and_then(|o| o.as_str())
                        .unwrap_or("")
                        .to_string();
                    let _ = events.send(EngineEvent::CommandCompleted {
                        item_id,
                        command,
                        status,
                        output,
                    });
                }
                "fileChange" => {
                    let status = item
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let _ = events.send(EngineEvent::FileChangeCompleted { item_id, status });
                }
                "agentMessage" => {
                    let text = item
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !text.is_empty() {
                        let _ = events.send(EngineEvent::AgentMessage { text });
                    }
                }
                _ => {}
            }
        }
        "item/commandExecution/outputDelta" => {
            let item_id = params
                .get("itemId")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let output = params
                .get("delta")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::CommandOutput { item_id, output });
        }
        "serverRequest/resolved" => {
            let rid = params
                .get("requestId")
                .and_then(|r| r.as_i64())
                .unwrap_or(-1);
            let _ = events.send(EngineEvent::ApprovalResolved { request_id: rid });
        }
        "windowsSandbox/setupCompleted" => {
            let success = params
                .get("success")
                .and_then(|s| s.as_bool())
                .unwrap_or(false);
            let mode = params
                .get("mode")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let error = params
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::SandboxSetupResult {
                success,
                mode,
                error,
            });
        }
        "turn/completed" => {
            let turn = params.get("turn").cloned().unwrap_or(Value::Null);
            let status = turn
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("completed")
                .to_string();
            let usage = turn
                .get("usage")
                .map(|u| serde_json::to_string(u).unwrap_or_default())
                .unwrap_or_default();
            let _ = events.send(EngineEvent::TurnCompleted { status, usage });
        }
        "error" => {
            let msg = params
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::Log {
                level: "error".into(),
                msg,
            });
        }
        "warning" => {
            let msg = params
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::Log {
                level: "warning".into(),
                msg,
            });
        }
        _ => {
            let _ = events.send(EngineEvent::Unknown {
                method: method.to_string(),
                payload: v.to_string(),
            });
        }
    }
}
