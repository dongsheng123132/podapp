//! chatlog 程序舱的无头验收。
//!
//! 它是第一个**要走宿主动作**的程序舱 —— 数据在 ~/.codex，而沙箱不让它读。
//! 所以这条测试同时验证那条链路：清单申报 → 运行时核对权限 → 宿主动作放行。

use podapp_runtime::{Capabilities, HeadlessHost, Invocation};
use serde_json::{json, Value};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
fn have_node() -> bool {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::var("PATH").ok().is_some_and(|p| {
        let sep = if cfg!(windows) { ';' } else { ':' };
        p.split(sep).any(|d| !d.is_empty() && std::path::Path::new(d).join(exe).exists())
    })
}

const SYSTEM_PROMPT: &str = "你是 Codex，这段是系统提示词不该被导出";

fn rollout() -> String {
    [
        json!({ "timestamp": "2026-07-27T08:42:13Z", "type": "session_meta",
                "payload": { "session_id": "s-1", "timestamp": "2026-07-27T08:42:12Z",
                             "cwd": r"C:\work", "cli_version": "0.146.0" } }),
        json!({ "type": "response_item", "payload": { "type": "message", "role": "developer",
                "content": [{ "type": "text", "text": SYSTEM_PROMPT }] } }),
        json!({ "timestamp": "2026-07-27T08:43:00Z", "type": "response_item",
                "payload": { "type": "message", "role": "user",
                             "content": [{ "type": "text", "text": "帮我把这张图切成九宫格" }] } }),
        json!({ "timestamp": "2026-07-27T08:43:05Z", "type": "response_item",
                "payload": { "type": "message", "role": "assistant", "content": "好的，用九宫格切图那个程序舱" } }),
    ]
    .iter()
    .map(|l| l.to_string() + "\n")
    .collect::<Vec<_>>()
    .concat()
}

/// 宿主把 codex 那组动作接上 —— 和浮舱 `DockHost::host_action` 里做的是同一件事。
fn host() -> HeadlessHost {
    HeadlessHost::with_host_actions(|id, input| {
        if id.starts_with("host.codex.") {
            podapp_codex::host_action(id, input)
        } else {
            Err(format!("capability_unavailable: {id}"))
        }
    })
}

fn run(action: &str, input: Value) -> Result<Value, String> {
    podapp_runtime::headless::invoke(
        &Invocation::new(action, input),
        &host(),
        &Capabilities::builtin(),
        None,
    )
}

fn in_sandbox(tag: &str, f: impl FnOnce()) {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let sb = std::env::temp_dir().join(format!("podapp-chatlog-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sb);
    let day = sb.join("codex/sessions/2026/07/27");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-2026-07-27T16-42-12-s-1.jsonl"), rollout()).unwrap();

    std::env::set_var("PODAPP_APPS_ROOT", &sb);
    std::env::set_var("PODAPP_ARTIFACTS_ROOT", sb.join("home"));
    std::env::set_var("CODEX_HOME", sb.join("codex")); // 绝不碰用户真实的对话
    podapp_runtime::install::install_from_path(&repo_root().join("pods/chatlog"), "test").unwrap();
    f();
    let _ = std::fs::remove_dir_all(&sb);
}

#[test]
fn listing_goes_through_the_host_action_chain() {
    if !have_node() { println!("跳过：本机没有 Node"); return; }
    in_sandbox("list", || {
        let out = run("app.chatlog.session.list", json!({})).unwrap_or_else(|e| panic!("失败: {e}"));
        assert_eq!(out["count"], 1);
        assert_eq!(out["sessions"][0]["title"], "帮我把这张图切成九宫格");
    });
}

#[test]
fn the_system_prompt_never_reaches_the_export() {
    if !have_node() { println!("跳过：本机没有 Node"); return; }
    in_sandbox("md", || {
        let out = run("app.chatlog.session.export", json!({ "session": "s-1" }))
            .unwrap_or_else(|e| panic!("失败: {e}"));
        assert_eq!(out["count"], 2, "只该有 user + assistant 两条");
        assert_eq!(out["format"], "markdown");

        // 导出的正文落在产物文件里，去盘上读出来验 —— 断言返回值不够，
        // 真正会被发出去的是那个文件
        let p = out["artifact"]["path"].as_str().expect("没有产物路径");
        let text = std::fs::read_to_string(p).unwrap();
        assert!(!text.contains("系统提示词"), "系统提示词泄漏进导出文件了");
        assert!(text.contains("帮我把这张图切成九宫格"));
        assert!(text.contains("🤖 Codex"));
        assert!(text.contains(r"C:\work"), "元信息该带上工作目录");
    });
}

#[test]
fn html_export_is_a_single_self_contained_file() {
    if !have_node() { println!("跳过：本机没有 Node"); return; }
    in_sandbox("html", || {
        let out = run("app.chatlog.session.export", json!({ "session": "s-1", "format": "html" })).unwrap();
        let text = std::fs::read_to_string(out["artifact"]["path"].as_str().unwrap()).unwrap();
        assert!(text.starts_with("<!doctype html>"));
        // 导出的东西要能直接发给别人：依赖 CDN 的页面在断网时是白的
        assert!(!text.contains("http://") && !text.contains("https://"), "HTML 里不该有外链");
        assert!(!text.contains("系统提示词"));
    });
}

#[test]
fn the_fallback_path_does_not_need_codex_at_all() {
    if !have_node() { println!("跳过：本机没有 Node"); return; }
    in_sandbox("fallback", || {
        // 把 CODEX_HOME 指到不存在的地方：上游改了目录结构时就是这个情形
        std::env::set_var("CODEX_HOME", std::env::temp_dir().join("no-codex-here-xyz"));
        let out = run("app.chatlog.session.export", json!({ "jsonl": rollout(), "title": "手动导入" }))
            .unwrap_or_else(|e| panic!("兜底路径也失败了: {e}"));
        assert_eq!(out["count"], 2);
        assert_eq!(out["title"], "手动导入");
        let text = std::fs::read_to_string(out["artifact"]["path"].as_str().unwrap()).unwrap();
        assert!(!text.contains("系统提示词"), "兜底路径也不许漏系统提示词");
    });
}

#[test]
fn asking_for_nothing_says_what_to_give() {
    if !have_node() { println!("跳过：本机没有 Node"); return; }
    in_sandbox("empty", || {
        let e = run("app.chatlog.session.export", json!({})).unwrap_err();
        assert!(e.contains("session") && e.contains("jsonl"), "实际: {e}");
    });
}
