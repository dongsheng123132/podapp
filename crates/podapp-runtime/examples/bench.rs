//! 量一次动作调用有多慢，并且把慢在哪儿拆开。
//!
//! # 为什么需要这个
//!
//! 「一键跑完一条流程」是不是可用，取决于**单步开销 × 步数**。而单步开销里
//! 有一块是不可压缩的（Node 启动），有几块是我们自己加上去的（每次重读清单、
//! 每次往磁盘写 runner 和入参）。不拆开量，改错地方的概率很大。
//!
//! 跑：`cargo run --release --example bench -p podapp-runtime`
//!
//! **必须 `--release`。** debug 下的 JSON 解析和路径处理慢好几倍，
//! 拿 debug 的数字做决定会把优化引到错的地方。
//!
//! 全程用隔离家目录，绝不碰真实 `~/.podapp`。

use podapp_runtime::{Capabilities, HeadlessHost, HostProfile, Invocation};
use std::time::{Duration, Instant};

const ROUNDS: usize = 12;
/// 最轻的一个官方动作：只读、无参数、不产出产物。
/// 量的是**开销**，不是这个动作的业务耗时。
const ACTION: &str = "app.memo.note.list";

fn stats(mut v: Vec<Duration>) -> (Duration, Duration, Duration) {
    v.sort();
    (v[0], v[v.len() / 2], v[v.len() - 1])
}

fn ms(d: Duration) -> String {
    format!("{:>6.1}ms", d.as_secs_f64() * 1000.0)
}

fn line(label: &str, v: Vec<Duration>) -> Duration {
    let (min, p50, max) = stats(v);
    println!("{label:<34} min {} 中位 {} max {}", ms(min), ms(p50), ms(max));
    p50
}

fn main() {
    let base = std::env::temp_dir().join(format!("podapp-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::env::set_var("PODAPP_HOME", &base);
    let _ = podapp_runtime::init(HostProfile::podapp("9.9.9"));

    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../pods/memo");
    if let Err(e) = podapp_runtime::install::install_from_path(&repo, "bench") {
        eprintln!("装不上 memo：{e}");
        return;
    }

    println!("== 拆开量：一次动作调用的开销 ==\n");

    // 1) Node 光启动要多久 —— 这是地板，我们压不下去
    let node = std::env::var("PODAPP_NODE").unwrap_or_else(|_| "node".into());
    let mut boot = Vec::new();
    for _ in 0..ROUNDS {
        let t = Instant::now();
        let ok = std::process::Command::new(&node)
            .args(["-e", "0"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if ok.is_err() {
            eprintln!("找不到 node，基线量不了");
            return;
        }
        boot.push(t.elapsed());
    }
    let boot_p50 = line("node -e 0（不可压缩的地板）", boot);

    // 2) 加上沙箱 flag 之后的启动 —— 权限模型自己也要钱
    let mut booted_sandbox = Vec::new();
    for _ in 0..ROUNDS {
        let t = Instant::now();
        let _ = std::process::Command::new(&node)
            .args([
                "--permission",
                &format!("--allow-fs-read={}", base.display()),
                "-e",
                "0",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        booted_sandbox.push(t.elapsed());
    }
    let sandbox_p50 = line("node --permission -e 0", booted_sandbox);

    // 3) 只读清单（每次 invoke 都重做一遍）
    let mut manifests = Vec::new();
    for _ in 0..ROUNDS {
        let t = Instant::now();
        let _ = podapp_runtime::manifest::action_specs();
        manifests.push(t.elapsed());
    }
    let specs_p50 = line("action_specs()（每次都重读盘）", manifests);

    // 4) 完整一次 invoke
    let host = HeadlessHost::new();
    let caps = Capabilities::builtin();
    let inv = Invocation::new(ACTION, serde_json::json!({}));
    // 先跑一次热身：第一次要建目录、OS 要把 node 读进文件缓存
    match podapp_runtime::headless::invoke(&inv, &host, &caps, None) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("动作跑不通，量不下去了：{e}");
            return;
        }
    }
    let mut whole = Vec::new();
    for _ in 0..ROUNDS {
        let t = Instant::now();
        let _ = podapp_runtime::headless::invoke(&inv, &host, &caps, None);
        whole.push(t.elapsed());
    }
    let whole_p50 = line("invoke() 一次完整调用", whole);

    println!("\n== 拆账（按中位数）==");
    let overhead = whole_p50.saturating_sub(sandbox_p50);
    println!(
        "  Node 启动           {}   ({:.0}%)",
        ms(sandbox_p50),
        sandbox_p50.as_secs_f64() / whole_p50.as_secs_f64() * 100.0
    );
    println!(
        "  沙箱 flag 的代价     {}",
        ms(sandbox_p50.saturating_sub(boot_p50))
    );
    println!("  读清单              {}", ms(specs_p50));
    println!(
        "  其余（写文件/IPC）   {}",
        ms(overhead.saturating_sub(specs_p50))
    );

    println!("\n== 一条流程要多久 ==");
    for steps in [2usize, 3, 5] {
        println!(
            "  {steps} 步   ≈ {}",
            ms(whole_p50 * steps as u32)
        );
    }
    println!(
        "\n结论线：单步 {} —— 3 步流程 {}。",
        ms(whole_p50),
        ms(whole_p50 * 3)
    );
    println!("人对「点一下」的容忍大约是 1 秒；超过就必须让界面显示进度，而不是干等。");

    std::env::remove_var("PODAPP_HOME");
    let _ = std::fs::remove_dir_all(&base);
}
