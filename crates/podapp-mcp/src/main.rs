//! `podapp-mcp` —— 把已装程序舱的动作当 MCP 工具端出去。JSON-RPC 2.0 over stdio。
//!
//! 装进 Claude Code / Codex 的 MCP 配置：
//!
//! ```json
//! { "mcpServers": { "podapp": { "command": "podapp-mcp" } } }
//! ```
//!
//! 之后装的每一个 `.pod` 都自动成为一个可调工具，不用改配置 ——
//! 工具表是每次 `tools/list` 现算的，不是启动时快照的。

use podapp_runtime::HostProfile;

fn main() {
    use std::io::{BufRead, Write};

    // 跟浮舱同一个宿主档案：同一个 ~/.podapp，看到的是同一批已装程序舱。
    let _ = podapp_runtime::init(HostProfile::podapp(env!("CARGO_PKG_VERSION")));

    // 能力集要和浮舱一致，否则「界面里能用的程序舱，AI 调就报 unknown_capability」。
    // **所以在这里不自己拼**：组装只有 podapp-host 一处，三个面同时拿到。
    // 这条注释以前就在，但当时只管住了能力，漏了宿主动作 —— 于是漂了。
    let caps = podapp_host::capabilities();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                // 日志走 stderr。往 stdout 里多写一个字节，对面的解析器就崩了。
                eprintln!("[podapp-mcp] 收到不是 JSON 的一行：{e}");
                continue;
            }
        };
        if let Some(reply) = podapp_mcp::handle(&msg, &caps) {
            let _ = writeln!(stdout, "{reply}");
            // stdio 传输必须每条都 flush —— 不 flush 的话对面在等，我们在缓冲，
            // 表现是「MCP 服务器没反应」而不是任何错误
            let _ = stdout.flush();
        }
    }
}
