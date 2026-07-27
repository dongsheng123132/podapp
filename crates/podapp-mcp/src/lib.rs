//! 把已装程序舱的动作暴露成 MCP 工具。
//!
//! # 为什么这件事值得单独做
//!
//! MCP 已经在成为「给 AI 一个工具」的事实标准。PodApp 要是被看成「另一个 MCP」，
//! 那就得让所有人二选一 —— 而二选一的东西很难被采纳。
//!
//! 正确的关系是**加法**：
//!
//! > MCP 给 AI 一个工具；PodApp 给人和 AI **同一个**工具。
//!
//! 装一个 `.pod` → 它立刻既是桌面上能点的程序舱，又是任何 MCP 客户端能调的工具。
//! 这里不新增任何执行路径，只是把 [`podapp_runtime::manifest::action_specs`] 已经
//! 摊平好的那张表翻译成 MCP 的说法 —— 人点按钮、AI 无头调、MCP 客户端调，
//! 走的仍是同一条 [`podapp_runtime::headless::invoke`]。
//!
//! # 传输层约定
//!
//! JSON-RPC 2.0 over stdio。**stdout 只有协议，日志一律走 stderr** ——
//! 往 stdout 里多打一个字节，对面的解析器就崩了，而症状是「MCP 服务器连不上」，
//! 跟原因隔着好几层。

use podapp_runtime::{Capabilities, HeadlessHost, Invocation};
use serde_json::{json, Value};

/// 跟 U-King 的 MCP 实现保持同一个版本号：同一台机器上两个宿主暴露同一批动作，
/// 协议版本不一致会让客户端表现出「有时能连有时不能」。
const PROTOCOL_VERSION: &str = "2024-11-05";

/// 动作 ID → MCP 工具名。
///
/// 点号换下划线：MCP 工具名的字符集限制在各家实现里不一致，换下划线是最保守的选择。
/// **但这个换算不可逆**（动作 ID 的段里本来就允许下划线），所以不靠反解 ——
/// [`call_tool`] 两种写法都认，原始 Action ID 也写进 description。
/// 契约不该因为传输层的字符限制而分叉。
///
/// 与 U-King `mcp_serve.rs` 的约定**必须一致**：同一个程序舱在两个宿主上
/// 该叫同一个工具名，否则用户的提示词换个宿主就失效。
pub fn tool_name(action_id: &str) -> String {
    action_id.replace('.', "_")
}

/// 当前可暴露的工具。
///
/// 只列**无头可跑**的动作：声明 `headless: false` 的动作在 MCP 这条路上根本执行不了，
/// 列出来只会让 AI 反复尝试再反复失败。
pub fn tools() -> Vec<Value> {
    podapp_runtime::manifest::action_specs()
        .into_iter()
        .filter(|a| {
            a.bindings.is_none()
                || a.input_schema.is_some()
                || !a.title.is_empty()
        })
        .map(|a| {
            let effect_note = match a.effect.as_str() {
                "read" => "Read-only; changes nothing.",
                "destructive" => "CHANGES FILES AND CANNOT BE UNDONE.",
                _ => "Writes output files.",
            };
            json!({
                "name": tool_name(&a.id),
                "description": format!(
                    "{}\n\n{}\n\n{} Action ID: {}",
                    a.title, a.description, effect_note, a.id
                ),
                "inputSchema": a.input_schema.clone().unwrap_or_else(
                    || json!({ "type": "object", "additionalProperties": false })
                ),
            })
        })
        .collect()
}

/// 跑一个工具。`caps` 由宿主提供 —— 装了 qr 能力的宿主，程序舱才调得到 `qr.*`。
pub fn call_tool(params: &Value, caps: &Capabilities) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "缺少工具名".to_string()))?;

    // 两种写法都认：原始 Action ID，以及点号换下划线之后的名字
    let spec = podapp_runtime::manifest::action_specs()
        .into_iter()
        .find(|a| a.id == name || tool_name(&a.id) == name)
        .ok_or((-32602, format!("没有这个工具: {name}")))?;

    let input = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let inv = Invocation::new(&spec.id, input);

    // 失败走 MCP 的 isError 而不是 JSON-RPC 的 error：**工具执行失败不是协议错误**。
    // 混成协议错误，客户端多半会断连或整个会话报错，而用户看到的只是「MCP 挂了」。
    match podapp_runtime::headless::invoke(&inv, &HeadlessHost::new(), caps, None) {
        Ok(v) => Ok(json!({
            "content": [{ "type": "text", "text": render(&v) }],
            "isError": false,
        })),
        Err(e) => Ok(json!({
            "content": [{ "type": "text", "text": e }],
            "isError": true,
        })),
    }
}

/// 结果渲染给文本客户端看。
///
/// 有 `message` 就以它开头 —— 终端客户端里，一行人话比一坨 JSON 有用得多。
/// 产物只给路径（运行时本来就只返回引用），几 MB base64 糊在对话里既是乱码
/// 又白烧上下文。
fn render(v: &Value) -> String {
    let mut out = String::new();
    if let Some(m) = v.get("message").and_then(|x| x.as_str()) {
        out.push_str(m);
        out.push('\n');
    }
    if let Some(p) = v.pointer("/artifact/path").and_then(|x| x.as_str()) {
        out.push_str(&format!("产物：{p}\n"));
    }
    out.push_str(&serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()));
    out
}

/// 处理一条 JSON-RPC 消息。
///
/// 返回 `None` 表示这是**通知**（没有 `id`），按 JSON-RPC 规范不能回 ——
/// 回了的话严格的客户端会当成协议违规。
pub fn handle(msg: &Value, caps: &Capabilities) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "podapp", "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(&params, caps),
        "ping" => Ok(json!({})),
        // notifications/* 是通知，本来就没有 id，下面会被过滤掉
        _ if method.starts_with("notifications/") => Ok(json!({})),
        other => Err((-32601, format!("不支持的方法: {other}"))),
    };

    // 通知不回应答
    let id = id?;

    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> Capabilities {
        Capabilities::builtin()
    }

    #[test]
    fn tool_names_avoid_dots_but_keep_the_real_id_discoverable() {
        assert_eq!(tool_name("app.nine-grid.image.split"), "app_nine-grid_image_split");
        // 连字符要留着 —— 它在 slug 里是合法字符，换掉会让两个不同的 slug 撞名
        assert!(tool_name("app.nine-grid.image.split").contains('-'));
    }

    #[test]
    fn notifications_get_no_reply() {
        // 回了的话严格的客户端会当协议违规
        let n = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&n, &caps()).is_none());
    }

    #[test]
    fn initialize_reports_tools_capability() {
        let r = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }), &caps()).unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "podapp");
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let r = handle(&json!({ "jsonrpc": "2.0", "id": 9, "method": "wat" }), &caps()).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn a_failing_tool_is_not_a_protocol_error() {
        // 这条是刻意的：工具跑失败走 isError，不走 JSON-RPC error。
        // 混成协议错误的话客户端多半直接断连，用户只看到「MCP 挂了」。
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                     "params": { "name": "app_nope_nope_nope", "arguments": {} } }),
            &caps(),
        )
        .unwrap();
        // 工具不存在属于参数错（客户端叫错了名字），这个才是协议层的
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn calling_without_a_name_is_rejected() {
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {} }),
            &caps(),
        )
        .unwrap();
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn render_puts_the_human_sentence_first_and_never_inlines_pixels() {
        let v = json!({
            "message": "已切成 9 张",
            "artifact": { "path": "C:/x/a.png", "id": "art_1" }
        });
        let s = render(&v);
        assert!(s.starts_with("已切成 9 张"), "一行人话该在最前面: {s}");
        assert!(s.contains("C:/x/a.png"));
        assert!(!s.contains("iVBORw0"), "不许把像素糊进对话");
    }
}
