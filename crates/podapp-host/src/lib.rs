//! 宿主能力与宿主动作的**唯一**组装处。
//!
//! # 为什么必须只有一处
//!
//! 泊舟有三个面：浮舱（GUI）、MCP（给 AI）、CLI（给调度器和脚本）。
//! 三个面必须看到**同一批动词** —— 这是「一份实现，多个面」的字面含义。
//!
//! 而在这个 crate 出现之前，它们各自组装：
//!
//! - 浮舱：`builtin() + qr`，宿主动作分发 codex / zip / cli
//! - MCP：`builtin() + qr`，宿主动作**一个都没有**（`HeadlessHost::new()`）
//!
//! 后果是实打实的：`chatlog` 申报了 `host.codex.session.*`、`nine-grid` 申报了
//! `host.zip.pack` —— 这两个官方 Pod 在浮舱里能用，AI 走 MCP 调就报
//! `capability_unavailable`。人能做的事和 AI 能做的事悄悄分了家，
//! **而两边都不报错，只有真去调那个动作的人才会撞上。**
//!
//! 所以：谁要加一个能力或宿主动作，只改这里。三个面自动一致。
//!
//! # 什么不在这里
//!
//! 弹文件对话框这类**只有界面才有**的能力留在浮舱自己那侧 ——
//! 无头模式弹不出对话框，硬塞进来只会得到一个永远失败的动词。
//!
//! AI 三条（`ai.chat` / `imageGen` / `imageEdit`）也不在这里：泊舟不接 AI 能力，
//! 那是决定不是待办（见 `AGENTS.md` 第四条不让步）。

use podapp_runtime::{Capabilities, HeadlessHost};
use serde_json::Value;

/// 所有面共用的能力集。
///
/// 加一个能力就在这里 `.with(...)` 一次，三个面同时拿到。
pub fn capabilities() -> Capabilities {
    Capabilities::builtin().with(podapp_qr::QrCapability)
}

/// 所有面共用的宿主动作分发。
///
/// 前缀分发而不是逐个 id 列举：一个能力 crate 加新动作时不用回来改这里，
/// 而**新增一个前缀必须回来改** —— 那正是该被看见的那种改动。
pub fn host_action(id: &str, input: Value) -> Result<Value, String> {
    if id.starts_with("host.codex.") {
        return podapp_codex::host_action(id, input);
    }
    if id.starts_with("host.zip.") {
        return podapp_zip::host_action(id, input);
    }
    // 本机已装的命令行工具。**这不是"接 AI"** —— 红线挡的是接入
    // （带 SDK、管密钥、背计费）；调用用户自己装好、自己配好的工具不是那些。
    if id.starts_with("host.cli.") {
        return podapp_cli::host_action(id, input);
    }
    Err(format!("capability_unavailable: 没有宿主动作 {id}"))
}

/// 无头宿主：给 MCP 和 CLI 用。
///
/// **必须带上宿主动作**。用 `HeadlessHost::new()` 的那个版本就是 parity 破口的来源。
pub fn headless_host() -> HeadlessHost {
    HeadlessHost::with_host_actions(host_action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **随包发布的每个 Pod 申报的宿主动作，都必须真的分发得到。**
    ///
    /// 这条是那个 parity 破口的守卫：以后谁加一个 Pod 并申报了新前缀的宿主动作，
    /// 忘了回来接上，这里就会红 —— 而不是等到用户在 MCP 那条路上撞见
    /// `capability_unavailable`，还以为是自己配错了。
    #[test]
    fn every_shipped_pod_can_reach_the_host_actions_it_declares() {
        let pods = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../pods");
        let mut checked = 0;
        for e in std::fs::read_dir(&pods).expect("读不到 pods/").flatten() {
            let manifest = e.path().join("podapp.json");
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let v: Value = serde_json::from_str(&text).expect("清单不是 JSON");
            let Some(ids) = v.pointer("/permissions/host_actions").and_then(|x| x.as_array())
            else {
                continue;
            };
            for id in ids.iter().filter_map(|x| x.as_str()) {
                checked += 1;
                // 入参故意给空：这里验的是**分发到不到**，不是动作本身跑不跑得通。
                // 所以只断言错误不是「没有这个宿主动作」。
                let err = host_action(id, json!({})).err().unwrap_or_default();
                assert!(
                    !err.contains("capability_unavailable"),
                    "{} 申报了 {id}，但宿主分发不到它（{err}）",
                    e.file_name().to_string_lossy()
                );
            }
        }
        // 一个都没查到，说明这条测试本身失效了（路径错、清单结构变了）——
        // 那比红更危险，因为它绿着
        assert!(checked > 0, "没查到任何申报的宿主动作，这条测试失效了");
    }

    #[test]
    fn an_unknown_prefix_is_still_refused() {
        let e = host_action("host.nope.do.it", json!({})).unwrap_err();
        assert!(e.contains("capability_unavailable"), "{e}");
    }

    /// 三个面拿到的能力名单必须一样。这里只能验「组装函数只有一个」，
    /// 所以顺带把名单钉住 —— 少了一个就会红。
    #[test]
    fn the_capability_set_is_assembled_in_one_place() {
        let names = capabilities().names();
        for want in ["image", "storage", "artifact", "qr"] {
            assert!(names.contains(&want), "能力集里少了 {want}: {names:?}");
        }
    }
}
