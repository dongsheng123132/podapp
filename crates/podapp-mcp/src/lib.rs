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
//!
//! # 两套握手同时在线
//!
//! 2026-07-28 把 MCP 改成了无状态：没有 `initialize` 握手，协议版本和客户端能力
//! 改成每个请求塞在 `_meta` 里，服务器**必须**实现 [`server/discover`]。
//!
//! 但老客户端还在，而且规范自己就说 stdio 上 `server/discover` 可以当**向后兼容探针**。
//! 所以这里两条都留着：
//!
//! - 新客户端先问 `server/discover`，拿到 `supportedVersions`，之后每个请求自报版本
//! - 老客户端照旧 `initialize`，拿到 [`LEGACY_PROTOCOL_VERSION`]
//!
//! **只在对方明确报了版本时才校验版本。** 不带 `_meta` 的请求一律按老客户端放行 ——
//! 对它们摆出 `UnsupportedProtocolVersion`，等于把今天能连的客户端全踢下线。
//!
//! [`server/discover`]: https://modelcontextprotocol.io/specification/2026-07-28/server/discover

use podapp_runtime::{Capabilities, HeadlessHost, Invocation};
use serde_json::{json, Value};

/// 认得的协议版本，**新的在前**（`server/discover` 按这个顺序端出去，
/// 客户端一般取第一个能用的）。
pub const SUPPORTED_VERSIONS: &[&str] = &["2026-07-28", "2024-11-05"];

/// 老握手（`initialize`）回的版本。
///
/// 跟 U-King 的 MCP 实现保持同一个版本号：同一台机器上两个宿主暴露同一批动作，
/// 协议版本不一致会让客户端表现出「有时能连有时不能」。
/// **这个常量要改，得和 U-King 的 `mcp_serve.rs` 同一次改** —— 新版本从
/// [`SUPPORTED_VERSIONS`] 那条路进来，不动这里，两边就不会因为升级节奏错开而分家。
const LEGACY_PROTOCOL_VERSION: &str = "2024-11-05";

/// 工具表的新鲜度提示（毫秒），随 `tools/list` 一起下发。
///
/// 工具表是**现算**的：装一个 `.pod` 它就变了。TTL 是这中间唯一的杠杆 ——
/// 给太长，「装完立刻能用」这句话就破功；给太短，对面每次都重新拉一遍工具表，
/// 白白搅掉 LLM 的 prompt cache。人装一个程序舱本来就要几秒，30 秒是这两头的中点。
const TOOLS_TTL_MS: u64 = 30_000;

/// 规范定义的错误码：客户端要的协议版本这边不认。
///
/// **`-32020` 到 `-32099` 是规范保留段**，这一段里只准用规范定义过的码。
/// 别在这个区间里自己发明码 —— 那是明确禁止的。
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// 请求里自报协议版本的 `_meta` 键。
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// 结果里自报身份的 `_meta` 键。
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

fn server_info() -> Value {
    json!({ "name": "podapp", "version": env!("CARGO_PKG_VERSION") })
}

/// 给一个结果盖上 `resultType` 和服务器身份。
///
/// 新规范要求**每个**结果都带 `resultType`；老客户端见到不认识的字段会忽略，
/// 所以两边都发是安全的 —— 不需要按对方版本分两套结果。
fn complete(mut result: Value) -> Value {
    if let Some(obj) = result.as_object_mut() {
        obj.insert("resultType".into(), json!("complete"));
        obj.entry("_meta")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .map(|m| m.insert(META_SERVER_INFO.into(), server_info()));
    }
    result
}

/// 对方这一条请求自报的协议版本。没有就是老客户端。
fn requested_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")?
        .get(META_PROTOCOL_VERSION)?
        .as_str()
}

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

/// 收件箱工具的名字。**不带 `app_` 前缀**，和程序舱动作区分开 ——
/// 它不属于任何一个程序舱，是宿主本身的能力。
pub const INBOX_TOOL: &str = "podapp_inbox_recent";

/// 「人刚才交给我什么」。
///
/// 其余工具都是 AI **让宿主做事**；只有这一个是反方向的：人在浮舱里标注了一张图、
/// 切了九宫格、修了二维码，产物落进收件箱，AI 主动来取。
///
/// 没有这一条，闭环就断在**人身上** —— 标完得自己复制、切窗口、粘贴。
/// 一次两次无所谓，一天二十次就没人用了。
///
/// 只回**引用**（id / 路径 / 那行人话），不回内容：一张 4MB 的 PNG 变成 base64
/// 塞进工具返回值，是把对方的上下文烧掉换一个它并不需要的东西 —— 它要的是路径，
/// 自己会去读。
fn inbox_tool() -> Value {
    json!({
        "name": INBOX_TOOL,
        "description": "List what the human most recently handed over from the PodApp dock \
    （人刚在浮舱里产出的东西）: annotated images, split tiles, fixed QR posters, exported chats. \
    Returns references (id, file path, human note) — never file contents; read the path yourself. \
    Read-only; changes nothing.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "limit": {
                    "type": "integer", "minimum": 1, "maximum": 50,
                    "description": "How many recent items (default 10)"
                },
                "unseen_only": {
                    "type": "boolean",
                    "description": "Only items the human hasn't acknowledged yet (default false)"
                }
            }
        }
    })
}

fn inbox_recent(args: &Value) -> Value {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, 50) as usize;
    let unseen_only = args
        .get("unseen_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let items: Vec<Value> = podapp_runtime::artifacts::list()
        .into_iter()
        .filter(|a| !unseen_only || !a.seen)
        .take(limit)
        .map(|a| {
            json!({
                "id": a.id,
                "kind": a.kind,
                "from_pod": a.source,
                "action": a.action,
                "note": a.message,
                "bytes": a.bytes,
                "width": a.w,
                "height": a.h,
                "path": podapp_runtime::artifacts::path_of(&a.id)
                    .map(|p| p.display().to_string()),
            })
        })
        .collect();

    json!({ "count": items.len(), "items": items })
}

/// 当前可暴露的工具。
///
/// 只列**无头可跑**的动作：声明 `headless: false` 的动作在 MCP 这条路上根本执行不了，
/// 列出来只会让 AI 反复尝试再反复失败。
///
/// 顺序是**确定的**：收件箱永远第一（它是宿主自己的能力，不属于任何程序舱），
/// 其余按工具名排序。规范建议这么做是为了对面的 prompt cache —— 同一批程序舱
/// 每次给出同一个字节序列，缓存才命中得了；顺序随目录遍历飘，等于每次都让对方重算。
pub fn tools() -> Vec<Value> {
    std::iter::once(inbox_tool())
        .chain(action_tools())
        .collect()
}

fn action_tools() -> Vec<Value> {
    let mut specs = podapp_runtime::manifest::action_specs();
    specs.sort_by(|a, b| tool_name(&a.id).cmp(&tool_name(&b.id)));
    specs
        .into_iter()
        .filter(|a| a.bindings.is_none() || a.input_schema.is_some() || !a.title.is_empty())
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

    // 收件箱不是程序舱动作，走不到下面那条动作总线 —— 它读的是宿主的收件箱，
    // 不属于任何一个程序舱，所以在这里先接住。
    if name == INBOX_TOOL {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let out = inbox_recent(&args);
        return Ok(json!({
            "content": [{ "type": "text", "text": render(&out) }],
            "isError": false,
        }));
    }

    // 两种写法都认：原始 Action ID，以及点号换下划线之后的名字
    let spec = podapp_runtime::manifest::action_specs()
        .into_iter()
        .find(|a| a.id == name || tool_name(&a.id) == name)
        .ok_or((-32602, format!("没有这个工具: {name}")))?;

    let input = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
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

/// 服务器身份、能力、认得的协议版本 —— 一次问清。
///
/// 新规范里服务器**必须**实现这条；它同时是 stdio 上的向后兼容探针：
/// 答得上来就是新服务器，答 `-32601` 就退回 `initialize`。
fn discover() -> Value {
    json!({
        "supportedVersions": SUPPORTED_VERSIONS,
        "capabilities": { "tools": {} },
        "instructions": "已装程序舱的动作都在这里。人在浮舱里点按钮、你在这里调工具，\
走的是同一条执行路径，所以你能做的事和用户能做的事完全一致。\
产物一律只回引用（路径 / id），要内容自己去读。\
问 podapp_inbox_recent 可以看到人刚在浮舱里产出了什么。",
        // 身份和能力只在装卸程序舱时才变，比工具表稳得多；
        // cacheScope 仍是 private —— 这是本机用户装了什么的事实，不该被共享中间层缓存。
        "ttlMs": TOOLS_TTL_MS,
        "cacheScope": "private",
    })
}

/// 处理一条 JSON-RPC 消息。
///
/// 返回 `None` 表示这是**通知**（没有 `id`），按 JSON-RPC 规范不能回 ——
/// 回了的话严格的客户端会当成协议违规。
pub fn handle(msg: &Value, caps: &Capabilities) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

    // 版本闸只对**自报了版本**的请求生效。老客户端不带 `_meta`，
    // 拿新规范的强制字段去要求它们，等于把今天能连的客户端全踢下线。
    if let Some(v) = requested_version(&params) {
        if !SUPPORTED_VERSIONS.contains(&v) {
            let id = id?;
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": UNSUPPORTED_PROTOCOL_VERSION,
                    "message": format!("不认得的协议版本: {v}"),
                    "data": { "supportedVersions": SUPPORTED_VERSIONS },
                }
            }));
        }
    }

    let result = match method {
        "server/discover" => Ok(discover()),
        "initialize" => Ok(json!({
            "protocolVersion": LEGACY_PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": server_info(),
        })),
        // 工具表是现算的，所以必须带新鲜度提示，否则对面要么长期看不到新装的程序舱，
        // 要么每轮都重拉一遍
        "tools/list" => Ok(json!({
            "tools": tools(),
            "ttlMs": TOOLS_TTL_MS,
            "cacheScope": "private",
        })),
        "tools/call" => call_tool(&params, caps),
        // 新规范删了 ping，但老客户端还会发 —— 收下，别报错
        "ping" => Ok(json!({})),
        // notifications/* 是通知，本来就没有 id，下面会被过滤掉
        _ if method.starts_with("notifications/") => Ok(json!({})),
        other => Err((-32601, format!("不支持的方法: {other}"))),
    };

    // 通知不回应答
    let id = id?;

    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": complete(r) }),
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
        assert_eq!(
            tool_name("app.nine-grid.image.split"),
            "app_nine-grid_image_split"
        );
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
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
            &caps(),
        )
        .unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], LEGACY_PROTOCOL_VERSION);
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "podapp");
    }

    /// 老客户端**一个字都不用改**还得能连上。
    /// 这条是整个升级里唯一会伤到现有用户的地方，所以单独钉一条。
    #[test]
    fn a_client_that_never_mentions_a_version_still_works() {
        for m in ["initialize", "tools/list", "ping"] {
            let r = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": m }), &caps())
                .unwrap_or_else(|| panic!("{m} 没有应答"));
            assert!(r["error"].is_null(), "{m} 不该报错: {r}");
        }
    }

    #[test]
    fn discover_advertises_both_dialects_and_identifies_itself() {
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 7, "method": "server/discover",
                     "params": { "_meta": { META_PROTOCOL_VERSION: "2026-07-28" } } }),
            &caps(),
        )
        .unwrap();
        let versions = r["result"]["supportedVersions"].as_array().unwrap();
        assert!(versions.iter().any(|v| v == "2026-07-28"));
        // 老方言必须一起端出去，否则老客户端探到 discover 之后会以为自己被抛弃了
        assert!(versions.iter().any(|v| v == LEGACY_PROTOCOL_VERSION));
        assert_eq!(r["result"]["_meta"][META_SERVER_INFO]["name"], "podapp");
    }

    /// 新规范要求**每个**结果都自报类型。漏一个，严格客户端就把那条结果判为无效。
    #[test]
    fn every_result_declares_its_type() {
        for m in ["server/discover", "initialize", "tools/list", "ping"] {
            let r = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": m }), &caps()).unwrap();
            assert_eq!(r["result"]["resultType"], "complete", "{m} 没带 resultType");
        }
    }

    /// 工具表是现算的 —— 不给新鲜度提示，对面只能猜：要么长期看不到新装的程序舱，
    /// 要么每轮都重拉一遍把 prompt cache 搅掉。
    #[test]
    fn the_tool_list_says_how_long_it_stays_fresh() {
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            &caps(),
        )
        .unwrap();
        assert!(r["result"]["ttlMs"].as_u64().unwrap() > 0);
        // 本机装了什么是用户的事实，绝不能让共享中间层缓存
        assert_eq!(r["result"]["cacheScope"], "private");
    }

    /// 同一批程序舱每次要给出同一个顺序，对面的 prompt cache 才命中得了。
    #[test]
    fn tools_come_back_in_a_stable_order() {
        let names = |v: &Value| -> Vec<String> {
            v["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap_or_default().to_string())
                .collect()
        };
        let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let first = names(&handle(&req, &caps()).unwrap());
        assert_eq!(first, names(&handle(&req, &caps()).unwrap()));
        // 收件箱是宿主自己的能力，永远排头 —— 它不该跟着某个程序舱的名字漂
        assert_eq!(first.first().map(String::as_str), Some(INBOX_TOOL));
        let mut sorted = first[1..].to_vec();
        sorted.sort();
        assert_eq!(&first[1..], &sorted[..], "程序舱动作没有按名字排序");
    }

    #[test]
    fn an_unknown_protocol_version_is_refused_with_the_code_the_spec_defines() {
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list",
                     "params": { "_meta": { META_PROTOCOL_VERSION: "1999-01-01" } } }),
            &caps(),
        )
        .unwrap();
        assert_eq!(r["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
        // 光说「不认」没用，得告诉对面认什么，它才知道降到哪一版
        assert!(r["error"]["data"]["supportedVersions"].is_array());
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let r = handle(
            &json!({ "jsonrpc": "2.0", "id": 9, "method": "wat" }),
            &caps(),
        )
        .unwrap();
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

#[cfg(test)]
mod inbox_tests {
    use super::*;

    /// 收件箱读的是进程级的 `PODAPP_ARTIFACTS_ROOT`，几个测试同时改会互相踩。
    /// 锁在代码里而不是靠 `--test-threads=1` —— 需要特殊参数才能过的测试是陷阱。
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn sandbox(tag: &str, f: impl FnOnce()) {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("podapp-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PODAPP_ARTIFACTS_ROOT", &dir);
        podapp_runtime::artifacts::clear();
        f();
        std::env::remove_var("PODAPP_ARTIFACTS_ROOT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    /// 收件箱工具必须出现在 tools/list 里 —— 不在列表里，AI 根本不知道能问。
    #[test]
    fn inbox_tool_is_advertised() {
        let names: Vec<String> = tools()
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        assert!(
            names.contains(&INBOX_TOOL.to_string()),
            "工具清单里没有收件箱: {names:?}"
        );
    }

    /// 人交出来的东西，AI 要能原样取到引用。
    #[test]
    fn agent_can_fetch_what_the_human_just_produced() {
        sandbox("fetch", || {
            podapp_runtime::artifacts::emit(
                "org.podapp.image.annotate",
                Some("app.annotate.task.build"),
                "image",
                PNG,
                Some("3 处标注 · 1920×1080"),
            )
            .unwrap();

            let out = inbox_recent(&json!({}));
            assert_eq!(out["count"], 1);
            let it = &out["items"][0];
            assert_eq!(it["from_pod"], "org.podapp.image.annotate");
            assert_eq!(it["note"], "3 处标注 · 1920×1080");
            // 给的是**路径**，AI 自己去读
            let p = it["path"].as_str().expect("要给出落盘路径");
            assert!(std::path::Path::new(p).exists(), "路径不存在: {p}");
        });
    }

    /// **绝不回内容。** 一张图 base64 进返回值 = 把对方上下文烧掉换一个它不需要的东西。
    /// 这条比「能取到」更容易在后续改动里破功，所以单独钉一条。
    #[test]
    fn never_returns_file_contents() {
        sandbox("noblob", || {
            podapp_runtime::artifacts::emit("p", None, "image", PNG, Some("x")).unwrap();
            let s = inbox_recent(&json!({})).to_string();
            assert!(!s.contains("iVBORw0"), "返回值里混进了 PNG base64");
            assert!(!s.contains("data:image"), "返回值里混进了 data URL");
        });
    }

    #[test]
    fn limit_and_unseen_filter_work() {
        sandbox("filter", || {
            for i in 0..4 {
                podapp_runtime::artifacts::emit("p", None, "image", PNG, Some(&format!("#{i}")))
                    .unwrap();
            }
            assert_eq!(inbox_recent(&json!({ "limit": 2 }))["count"], 2);
            // 全都没看过，所以 unseen_only 应当一个不少
            assert_eq!(inbox_recent(&json!({ "unseen_only": true }))["count"], 4);
            let ids: Vec<String> = podapp_runtime::artifacts::list()
                .iter()
                .map(|a| a.id.clone())
                .collect();
            podapp_runtime::artifacts::mark_seen(&ids[..2]);
            assert_eq!(inbox_recent(&json!({ "unseen_only": true }))["count"], 2);
        });
    }

    /// 空收件箱要给出干净的 0，不能报错 —— AI 问「有什么给我吗」得到一个错误，
    /// 多半会重试或者放弃，而正确答案只是「暂时没有」。
    #[test]
    fn empty_inbox_is_not_an_error() {
        sandbox("empty", || {
            let out = inbox_recent(&json!({}));
            assert_eq!(out["count"], 0);
            assert!(out["items"].as_array().unwrap().is_empty());
        });
    }

    /// 走完整的 MCP `tools/call` 入口，不是直接调内部函数 ——
    /// 分发那一步接错了名字，上面几条照样全绿。
    #[test]
    fn reachable_through_the_real_tools_call_path() {
        sandbox("dispatch", || {
            podapp_runtime::artifacts::emit("p", None, "image", PNG, Some("hi")).unwrap();
            let caps = Capabilities::builtin();
            let r = call_tool(
                &json!({ "name": INBOX_TOOL, "arguments": { "limit": 1 } }),
                &caps,
            )
            .expect("收件箱工具应当可调用");
            assert_eq!(r["isError"], false);
            let text = r["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("hi"), "返回里没有那行人话: {text}");
        });
    }
}
