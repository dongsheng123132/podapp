//! `examples/flows/` 里每一份案例都必须在**这一版官方 Pod 上**验得过。
//!
//! # 为什么这条测试比案例本身重要
//!
//! 案例的读者主要是 AI：用户说一句小需求，AI 照着案例生成一份流程 JSON。
//! **所以一份腐坏的案例比没有案例更糟** —— 它会被照着抄。
//!
//! 而案例腐坏是必然的：官方 Pod 改一个动作 ID、给某个参数加一条 required，
//! 案例就错了，而 markdown 里的案例不会有人去重跑。
//!
//! 这条测试把 `pods/` 全装进隔离家目录，再对每份案例跑 `check` —— 动作在不在、
//! 必填给没给、`$prev` 用得对不对，全在这里被钉住。
//!
//! # 为什么只 check 不 run
//!
//! 跑起来要真图、真会话、真数据，那些不该进仓库。而案例最容易坏的部分恰好是
//! `check` 管的那部分，不是运行时的部分。

use std::path::PathBuf;

/// 改进程级环境变量，跟别的测试抢同一份状态 —— 锁在代码里，
/// 不靠 `--test-threads=1`（需要特殊参数才能过的测试是陷阱）。
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_example_flow_checks_clean_against_the_shipped_pods() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = std::env::temp_dir().join(format!("podapp-examples-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::env::set_var("PODAPP_HOME", &home);
    // 宿主版本给一个很高的值：清单里的 min_host_version 不该让这条测试
    // 在每次版本号没跟上时莫名失败
    let _ = podapp_runtime::init(podapp_runtime::HostProfile::podapp("9.9.9"));

    let mut installed = 0;
    for e in std::fs::read_dir(repo().join("pods"))
        .expect("读不到 pods/")
        .flatten()
    {
        if e.path().is_dir()
            && podapp_runtime::install::install_from_path(&e.path(), "test").is_ok()
        {
            installed += 1;
        }
    }

    let dir = repo().join("examples/flows");
    let mut checked = 0;
    let mut failures: Vec<String> = Vec::new();

    for e in std::fs::read_dir(&dir).expect("读不到 examples/flows/").flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        checked += 1;
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&p).expect("读不了案例");
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                failures.push(format!("{name}: 不是能读的 JSON —— {err}"));
                continue;
            }
        };
        match podapp_flow::parse(&v) {
            Err(err) => failures.push(format!("{name}: 形状不对 —— {err}")),
            Ok(flow) => {
                let problems = podapp_flow::check(&flow);
                if !problems.is_empty() {
                    failures.push(format!("{name}: {}", problems.join("；")));
                }
            }
        }
    }

    std::env::remove_var("PODAPP_HOME");
    let _ = std::fs::remove_dir_all(&home);

    // 两条护栏：装没装上、查没查到。任意一条为 0，这测试就等于什么都没验，
    // **而那比红更危险，因为它绿着**。
    assert!(installed > 0, "一个官方 Pod 都没装上，这条测试失效了");
    assert!(
        checked > 0,
        "examples/flows/ 里一份案例都没查到，这条测试失效了"
    );
    assert!(
        failures.is_empty(),
        "案例在这一版官方 Pod 上验不过（改了动作 ID 或必填参数？）：\n  {}",
        failures.join("\n  ")
    );
    eprintln!("[案例] {installed} 个 Pod / {checked} 份案例，全部验过");
}
