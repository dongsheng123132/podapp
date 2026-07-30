//! 版本号在三处，必须一致。
//!
//! # 为什么做成测试而不是文档提醒
//!
//! U-King 的 `AGENTS.md` 里写着「**版本号四处同步**（发版必同时改，否则自升级判断
//! 或显示对不上）」—— 写得很清楚，**然后还是漂了**：同一份文档里一处说四处、
//! 一处说三处，而实际有几处只有翻代码才知道。
//!
//! 泊舟这边也已经漂过一次：`tauri.conf.json` 和 `Cargo.toml` 升到 0.2.0 的时候，
//! `package.json` 还留在 0.1.0。
//!
//! 提醒挡不住漂移。**同一个事实存在几份就会漂几份**（宪法第 8 条），
//! 而唯一能挡住的是让不一致变成红色。
//!
//! # 为什么不干脆只留一处
//!
//! 试过就知道不行：Tauri 打包读 `tauri.conf.json`，Cargo 读 `Cargo.toml`，
//! npm 工具链读 `package.json`。三个生态各要一份，删不掉。
//! 能做的是让它们**必须**一样。

use std::path::PathBuf;

fn dock() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 从一份 JSON 里抠顶层 `version`。
///
/// 不引 serde_json 之外的东西，也不为这点事去建结构体。
fn json_version(path: &PathBuf) -> String {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
    let v: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是 JSON：{e}", path.display()));
    v.get("version")
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| panic!("{} 里没有顶层 version", path.display()))
        .to_string()
}

#[test]
fn the_version_is_the_same_in_every_place_that_carries_it() {
    // 这一份来自 Cargo.toml（编译时注入），所以它天然是准的
    let cargo = env!("CARGO_PKG_VERSION").to_string();
    let conf = json_version(&dock().join("tauri.conf.json"));
    let pkg = json_version(&dock().join("../package.json"));

    assert_eq!(
        conf, cargo,
        "tauri.conf.json({conf}) 和 Cargo.toml({cargo}) 版本不一致 —— \
Tauri 打包读前者、Cargo 读后者，对不上会让安装包版本和程序自报版本分家"
    );
    assert_eq!(
        pkg, cargo,
        "package.json({pkg}) 和 Cargo.toml({cargo}) 版本不一致 —— \
升版本时最容易漏的就是这一份（泊舟已经漏过一次）"
    );
}

/// 自升级的端点里**必须有一个国内可达的**。
///
/// U-King 用血换来的一条：`u-king.org` 在 Vercel 上，**国内直连不通**，
/// 所以它的国内入口是另一个域名。泊舟的 `podapp.net` 同样是 Vercel
/// （响应头 `Server: Vercel`），GitHub 在国内也常不通 ——
/// 只配这两个等于国内用户永远更新不到，而**这件事在开发机上永远看不出来**
/// （开发机有代理，宪法第 4 条）。
///
/// 这条测试不验可达性（那要联网，而且开发机的结果不算），只验**清单里留了那个位置**。
#[test]
fn the_updater_keeps_a_mainland_reachable_endpoint() {
    let text = std::fs::read_to_string(dock().join("tauri.conf.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let eps: Vec<String> = v
        .pointer("/plugins/updater/endpoints")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect())
        .unwrap_or_default();

    assert!(!eps.is_empty(), "自升级一个端点都没配");
    // Vercel 和 GitHub 都不算国内可达
    let mainland: Vec<&String> = eps
        .iter()
        .filter(|e| !e.contains("podapp.net") && !e.contains("github.com"))
        .collect();
    assert!(
        !mainland.is_empty(),
        "端点里只剩 Vercel(podapp.net) 和 GitHub，国内用户更新不到。\
现在是：{eps:?}。（我删过一次，因为它当时 404 —— 404 是没上线，不是不该存在。）"
    );
    // 国内那个该排在前面：Tauri 按顺序试，排后面等于让国内用户先白等两次超时
    assert!(
        !eps[0].contains("podapp.net") && !eps[0].contains("github.com"),
        "第一个端点该是国内可达的那个，现在是 {}",
        eps[0]
    );
}
