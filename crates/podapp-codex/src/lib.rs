//! 只读访问 Codex 的会话记录。
//!
//! # 为什么这必须是**宿主动作**，不是桥上的能力
//!
//! 程序舱的动作模块跑在开了 Node 权限模型的子进程里，只准读它自己的目录 ——
//! 它**碰不到** `~/.codex`，这是故意的。所以「读用户的对话历史」这件事只能由宿主做。
//!
//! 而且它该走**宿主动作**（`permissions.host_actions`）而不是桥上的能力：
//! 能力（`image.*` 那类）对所有程序舱一律开放，适合无害的原语；
//! 对话历史是敏感数据，必须在清单里逐条申报，装包时明明白白列给用户看
//! （「调用宿主动作 host.codex.session.read」）。
//!
//! # 对上游内部结构的态度
//!
//! `~/.codex/sessions/` 的布局是 Codex 的内部实现，随时会变。U-King 的 `codex.rs`
//! 为此删过一整块 computer-use 探测 —— 追着上游私有路径跑，代码永远在追，还常误判。
//!
//! 这里的做法是：**只依赖两件很稳的事** —— 目录按 `年/月/日` 分层、每行一条 JSON。
//! 认不出的记录类型一律跳过而不是报错（上游加字段是常态）；
//! 认不出整个目录时给一句人能看懂的话，让调用方走「拖入已导出文件」那条兜底路。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 会话根目录。`CODEX_HOME` 顶掉它 —— 测试靠这个绝不碰用户真实的对话。
pub fn sessions_root() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_HOME") {
        return PathBuf::from(p).join("sessions");
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".codex").join("sessions")
}

/// 一次会话的摘要。
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub started: String,
    pub cwd: String,
    /// 第一句用户说的话，截断后当标题 —— 光有 uuid 的列表没法选
    pub title: String,
    pub path: PathBuf,
    pub turns: usize,
}

impl SessionInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "started": self.started, "cwd": self.cwd,
            "title": self.title, "turns": self.turns,
            "path": self.path.display().to_string(),
        })
    }
}

fn read_lines(p: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(p) else { return vec![] };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        // 坏行跳过而不是整份放弃：会话文件可能正被 Codex 追写，最后一行可能是半条
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// 这条记录是不是一句「人看得见的对话」，是的话返回 (角色, 文本)。
///
/// 刻意**丢掉 `developer` 角色**：那是系统提示词，又长又不是用户的对话，
/// 导出到文档里既是噪音又可能带出不该外传的内容。
fn message_of(rec: &Value) -> Option<(String, String)> {
    let p = rec.get("payload")?;
    let role = p.get("role")?.as_str()?;
    if role == "developer" || role == "system" {
        return None;
    }
    let content = p.get("content")?;
    // content 可能是字符串，也可能是 [{type,text}] —— 两种都认，
    // 因为上游在不同版本里两种都出现过
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|c| {
                c.get("text")
                    .and_then(|t| t.as_str())
                    .map(String::from)
                    .or_else(|| c.as_str().map(String::from))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| (role.to_string(), text.to_string()))
}

fn summarize(path: &Path) -> Option<SessionInfo> {
    let lines = read_lines(path);
    let meta = lines.iter().find(|r| r.get("type").and_then(|t| t.as_str()) == Some("session_meta"))?;
    let p = meta.get("payload")?;

    let msgs: Vec<(String, String)> = lines.iter().filter_map(message_of).collect();
    let title = msgs
        .iter()
        .find(|(r, _)| r == "user")
        .map(|(_, t)| {
            let one: String = t.lines().next().unwrap_or("").chars().take(60).collect();
            one
        })
        .unwrap_or_else(|| "(没有用户消息)".into());

    Some(SessionInfo {
        id: p.get("session_id").or_else(|| p.get("id")).and_then(|v| v.as_str()).unwrap_or("").into(),
        started: p.get("timestamp").and_then(|v| v.as_str()).unwrap_or("").into(),
        cwd: p.get("cwd").and_then(|v| v.as_str()).unwrap_or("").into(),
        title,
        path: path.to_path_buf(),
        turns: msgs.len(),
    })
}

/// 列出会话，最新的在前。`limit` 是硬上限 —— 一台老机器上可能有上千个会话，
/// 一次全端出去会让界面卡住，而卡住看起来像程序坏了。
pub fn list_sessions(limit: usize) -> Vec<SessionInfo> {
    let root = sessions_root();
    let mut files = vec![];
    // 目录结构是 年/月/日/rollout-*.jsonl；不递归靠猜层数，直接走到底
    collect_jsonl(&root, &mut files, 0);
    // 文件名里带 ISO 时间戳，字典序即时间序 —— 不用逐个读文件就能排
    files.sort();
    files.reverse();
    files.iter().take(limit).filter_map(|p| summarize(p)).collect()
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_jsonl(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

/// 读一次会话的对话内容。
///
/// **路径必须落在会话根目录之内。** 这个函数的入参来自程序舱，
/// 而程序舱是第三方代码 —— 不夹这一道，它就能拿这个宿主动作去读任意文件，
/// 那样沙箱就白做了。
pub fn read_session(path_or_id: &str) -> Result<Value, String> {
    let root = sessions_root().canonicalize().map_err(|_| {
        "找不到 Codex 的会话目录（~/.codex/sessions）—— 可能没装 Codex，\
         或者版本变了。可以改用「拖入已导出的对话文件」那条路。"
            .to_string()
    })?;

    // 既认完整路径，也认 session id：调用方拿到的列表里两样都有
    let candidate = if path_or_id.contains(['/', '\\']) {
        PathBuf::from(path_or_id)
    } else {
        list_sessions(500)
            .into_iter()
            .find(|s| s.id == path_or_id)
            .map(|s| s.path)
            .ok_or_else(|| format!("没有这个会话: {path_or_id}"))?
    };

    let real = candidate
        .canonicalize()
        .map_err(|e| format!("读不到会话文件: {e}"))?;
    if !real.starts_with(&root) {
        return Err("拒绝：会话路径不在 ~/.codex/sessions 之内".into());
    }

    let lines = read_lines(&real);
    let meta = lines
        .iter()
        .find(|r| r.get("type").and_then(|t| t.as_str()) == Some("session_meta"))
        .and_then(|r| r.get("payload"))
        .cloned()
        .unwrap_or(Value::Null);

    let messages: Vec<Value> = lines
        .iter()
        .filter_map(|r| {
            let (role, text) = message_of(r)?;
            Some(json!({
                "role": role,
                "text": text,
                "at": r.get("timestamp").and_then(|v| v.as_str()).unwrap_or(""),
            }))
        })
        .collect();

    Ok(json!({
        "id": meta.get("session_id").or_else(|| meta.get("id")).cloned().unwrap_or(Value::Null),
        "started": meta.get("timestamp").cloned().unwrap_or(Value::Null),
        "cwd": meta.get("cwd").cloned().unwrap_or(Value::Null),
        "cli_version": meta.get("cli_version").cloned().unwrap_or(Value::Null),
        "count": messages.len(),
        "messages": messages,
    }))
}

/// 宿主动作分发。宿主把它接到 `HostBridge::host_action` 上。
pub fn host_action(id: &str, input: Value) -> Result<Value, String> {
    match id {
        "host.codex.session.list" => {
            let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).clamp(1, 500);
            let items: Vec<Value> = list_sessions(limit as usize).iter().map(|s| s.to_json()).collect();
            Ok(json!({ "count": items.len(), "sessions": items }))
        }
        "host.codex.session.read" => {
            let s = input
                .get("session")
                .and_then(|v| v.as_str())
                .ok_or("invalid_input: 缺少 session（id 或路径）")?;
            read_session(s)
        }
        other => Err(format!("capability_unavailable: 没有这个宿主动作 {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 造一个假的 CODEX_HOME，绝不碰用户真实的对话记录。
    fn sandbox(f: impl FnOnce(&Path)) {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("podapp-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let day = dir.join("sessions/2026/07/27");
        std::fs::create_dir_all(&day).unwrap();
        std::env::set_var("CODEX_HOME", &dir);

        let lines = [
            json!({ "timestamp": "2026-07-27T08:42:13Z", "type": "session_meta",
                    "payload": { "session_id": "abc-123", "timestamp": "2026-07-27T08:42:12Z",
                                 "cwd": "C:\\work", "cli_version": "0.146.0" } }),
            // 系统提示词：必须被丢掉
            json!({ "type": "response_item",
                    "payload": { "type": "message", "role": "developer",
                                 "content": [{ "type": "text", "text": "你是 Codex，这段是系统提示词" }] } }),
            json!({ "timestamp": "2026-07-27T08:43:00Z", "type": "response_item",
                    "payload": { "type": "message", "role": "user",
                                 "content": [{ "type": "text", "text": "帮我切个九宫格" }] } }),
            json!({ "timestamp": "2026-07-27T08:43:05Z", "type": "response_item",
                    "payload": { "type": "message", "role": "assistant",
                                 "content": "好的，我来切" } }),
            // 认不出的类型：跳过，别报错
            json!({ "type": "world_state", "payload": { "whatever": 1 } }),
        ];
        let body: String =
            lines.iter().map(|l| l.to_string() + "\n").collect::<Vec<_>>().concat()
            + "{ 这是半条被截断的行\n"; // 追写中的文件常见
        std::fs::write(day.join("rollout-2026-07-27T16-42-12-abc-123.jsonl"), body).unwrap();

        f(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lists_sessions_with_a_usable_title() {
        sandbox(|_| {
            let v = host_action("host.codex.session.list", json!({})).unwrap();
            assert_eq!(v["count"], 1);
            let s = &v["sessions"][0];
            assert_eq!(s["id"], "abc-123");
            // 只有 uuid 的列表没法选，标题取第一句用户说的话
            assert_eq!(s["title"], "帮我切个九宫格");
            assert_eq!(s["turns"], 2, "developer 那条不该算进对话轮数");
        });
    }

    #[test]
    fn the_system_prompt_never_leaves_the_machine() {
        // 系统提示词又长又不是用户的对话，导进文档既是噪音又可能带出不该外传的内容
        sandbox(|_| {
            let v = host_action("host.codex.session.read", json!({ "session": "abc-123" })).unwrap();
            let s = v.to_string();
            assert!(!s.contains("系统提示词"), "developer 角色泄漏了");
            assert_eq!(v["count"], 2);
            assert_eq!(v["messages"][0]["role"], "user");
            assert_eq!(v["messages"][1]["role"], "assistant");
            // content 两种写法（字符串 / 数组）都要认，上游两种都出现过
            assert_eq!(v["messages"][1]["text"], "好的，我来切");
        });
    }

    #[test]
    fn a_half_written_line_does_not_lose_the_whole_session() {
        // 会话文件可能正被 Codex 追写，最后一行是半条 —— 整份放弃是错的
        sandbox(|_| {
            let v = host_action("host.codex.session.read", json!({ "session": "abc-123" })).unwrap();
            assert_eq!(v["count"], 2, "坏行该跳过，不该拖垮整份");
        });
    }

    #[test]
    fn reading_outside_the_sessions_root_is_refused() {
        // 入参来自第三方程序舱。不夹这道，宿主动作就成了任意文件读取器，沙箱白做。
        sandbox(|dir| {
            let outside = dir.join("secret.txt");
            std::fs::write(&outside, "不该被读到").unwrap();
            let e = read_session(outside.to_str().unwrap()).unwrap_err();
            assert!(e.contains("拒绝") || e.contains("不在"), "实际: {e}");
        });
    }

    #[test]
    fn an_unknown_host_action_is_refused() {
        assert!(host_action("host.codex.session.delete", json!({})).is_err());
    }

    #[test]
    fn a_missing_codex_says_what_to_do_instead() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CODEX_HOME", std::env::temp_dir().join("definitely-not-codex-xyz"));
        let e = read_session("whatever").unwrap_err();
        // 报错要给出下一步，不能只说「失败」
        assert!(e.contains("拖入") || e.contains("没装"), "实际: {e}");
    }
}
