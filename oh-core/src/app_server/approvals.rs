//! app-server 服务端请求处理：审批、工具调用、时间等需要回包的请求。

use crate::model::EngineEvent;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// 处理服务端发起的请求（带 id，需要回包）：审批、工具、时间等。
pub(crate) async fn handle_server_request(
    id: i64,
    method: &str,
    v: &Value,
    events: &mpsc::UnboundedSender<EngineEvent>,
    out: &mpsc::UnboundedSender<String>,
    server_requests: &Arc<Mutex<HashMap<i64, String>>>,
) {
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    server_requests.lock().await.insert(id, method.to_string());
    match method {
        "item/commandExecution/requestApproval" => {
            let command = params
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let cwd = params
                .get("cwd")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let reason = params
                .get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            let item_id = params
                .get("itemId")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let _ = events.send(EngineEvent::ApprovalRequest {
                request_id: id,
                kind: "command".into(),
                item_id,
                command,
                cwd,
                reason,
                changes: String::new(),
            });
        }
        "item/fileChange/requestApproval" => {
            let item_id = params
                .get("itemId")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let reason = params
                .get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            let changes = params
                .get("changes")
                .map(|c| serde_json::to_string(c).unwrap_or_default())
                .unwrap_or_default();
            let _ = events.send(EngineEvent::ApprovalRequest {
                request_id: id,
                kind: "fileChange".into(),
                item_id,
                command: String::new(),
                cwd: String::new(),
                reason,
                changes,
            });
        }
        "item/tool/requestUserInput" => {
            // 不支持，自动拒绝
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "value": null } });
            let _ = out.send(msg.to_string());
        }
        "mcpServer/elicitation/request" => {
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "action": "decline", "content": null } });
            let _ = out.send(msg.to_string());
        }
        "item/tool/call" => {
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "contentItems": [], "success": false } });
            let _ = out.send(msg.to_string());
        }
        "currentTime/read" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": { "currentTimeAt": now } });
            let _ = out.send(msg.to_string());
        }
        _ => {
            // 未知请求：失败关闭
            let msg = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "unsupported by office harness" } });
            let _ = out.send(msg.to_string());
        }
    }
}
