//! 浮舱这个宿主给程序舱提供的能力。
//!
//! 没接的那几条给的是明确的 `capability_unavailable`，不是半通不通的实现 ——
//! 后者会让程序舱作者以为自己写对了，直到某天发现结果一直是空的。
//!
//! # AI 三条：**永远不接**，这是决定不是待办
//!
//! `ai_chat` / `ai_image_generate` / `ai_image_edit` 在这里会一直返回错误。
//! 不是"还没排到"，是**这个产品不做 AI 能力接入**：
//!
//! - 接一条 AI 能力，就要带一个 SDK、一套密钥管理、一份计费口径、一条要跟着上游
//!   改版的代码路径。浮舱现在整包 2.3 MB，接完就不是这个东西了。
//! - **用户机器上已经有 AI 了** —— Codex、Claude Code 就装在旁边。它们通过 MCP
//!   调浮舱的动作，浮舱不需要反过来调模型。
//! - 分工是清楚的：**浮舱负责采集和完成（确定性），AI 负责生成和理解。**
//!   录音落成一个 wav、录屏落成一个 mp4、图集校验出一份报告 —— 这些是确定性的；
//!   之后要拿它做什么，是旁边那个 agent 的事，它自己会来收件箱取。
//!
//! 所以：**要"AI 能力"的 Pod，正确做法是让 AI 来调它，而不是让它去调 AI。**
//! 谁想把这三条接上，先回来读这一段。

use podapp_runtime::HostBridge;
use serde_json::Value;

pub struct DockHost;

/// AI 三条统一的回话。写成一处是为了让"这是决定"这件事只有一个说法 ——
/// 三处各写一句，改口风的时候就会只改到其中一句。
const NO_AI: &str = "capability_denied: 泊舟不接 AI 能力（这是设计决定，不是待办）。\
你机器上的 Codex / Claude Code 可以通过 MCP 调用泊舟的动作 —— \
让 AI 来调这个动作，而不是让这个动作去调 AI。";

impl HostBridge for DockHost {
    fn ai_image_edit(&self, _a: &Value) -> Result<Value, String> {
        Err(NO_AI.into())
    }
    fn ai_image_generate(&self, _a: &Value) -> Result<Value, String> {
        Err(NO_AI.into())
    }
    fn ai_chat(&self, _a: &Value) -> Result<Value, String> {
        Err(NO_AI.into())
    }
    fn file_save(&self, _n: &str, _d: &str) -> Result<Value, String> {
        Err("capability_unavailable: 另存为对话框还没接".into())
    }
    fn file_open(&self, _f: &[String]) -> Result<Value, String> {
        Err("capability_unavailable: 打开对话框还没接".into())
    }
    /// 宿主动作。**权限闸在上游**：运行时已经核对过调用方在
    /// `permissions.host_actions` 里申报了这个 ID，这里不必再查一遍
    /// —— 查两遍的坏处是两处规则会慢慢不一致。
    fn host_action(&self, id: &str, input: Value) -> Result<Value, String> {
        if id.starts_with("host.codex.") {
            return podapp_codex::host_action(id, input);
        }
        if id.starts_with("host.zip.") {
            return podapp_zip::host_action(id, input);
        }
        Err(format!("capability_unavailable: 浮舱没有宿主动作 {id}"))
    }
}
