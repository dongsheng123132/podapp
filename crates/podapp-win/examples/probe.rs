//! 探针：`cargo run -p podapp-win --example probe`
//!
//! 在真机上回答一个问题：**现在能不能找到 Codex 的主窗口，它在哪。**
//! 浮舱吸附对不对，最终只能这样验 —— 单元测试证明不了「屏幕上那个窗口是不是它」。

use podapp_win::{find_host_window, pids_of, HostApp, CODEX_APP};

/// 浮舱自己。用来验证「它真的停在算出来的地方」—— 这是唯一能客观回答
/// 「吸附对不对」的办法，截图和肉眼都不行。
const PODAPP_DOCK: HostApp = HostApp {
    label: "PodApp 浮舱",
    path_marker: "podapp-dock",
    exe_names: &["podapp-dock.exe", "podapp.exe"],
};

/// `--verify`：把浮舱**实际**落点和 `dock::place` **算出来**的落点比一比。
fn verify() -> i32 {
    // 用 find_windows_of 而不是 find_host_window：浮舱收起时只有 64px 宽，
    // 会被「像个主窗口」那把尺子筛掉。挑最大的那个（Tauri 可能另有隐藏辅助窗）。
    let Some(dock) = podapp_win::find_windows_of(&PODAPP_DOCK)
        .into_iter()
        .max_by_key(|w| w.rect.w * w.rect.h)
    else {
        println!("FAIL 浮舱没在跑 —— 先启动 podapp-dock.exe");
        return 1;
    };
    let host = find_host_window(&CODEX_APP);
    let work = podapp_win::work_area(host.as_ref().map(|h| h.hwnd).or(Some(dock.hwnd)));

    // 浮舱启动时是收起态
    let want = podapp_win::dock::place(
        host.as_ref().map(|h| h.rect),
        work,
        false,
        podapp_win::dock::Metrics::platform(),
    );

    println!("宿主   ：{:?}", host.as_ref().map(|h| h.rect));
    println!("工作区 ：{work:?}");
    println!("应该在 ：{:?}", want.rect);
    println!("实际在 ：{:?}", dock.rect);

    // 允许 1px 误差：窗口边框取整在某些缩放比下会差一个像素，那不是吸附错了
    let d = |a: i32, b: i32| (a - b).abs() <= 1;
    let ok = d(dock.rect.x, want.rect.x)
        && d(dock.rect.y, want.rect.y)
        && d(dock.rect.w, want.rect.w)
        && d(dock.rect.h, want.rect.h);
    println!(
        "\n{}",
        if ok {
            "PASS 浮舱停在算出来的位置"
        } else {
            "FAIL 浮舱位置和计算不符"
        }
    );
    i32::from(!ok)
}

fn main() {
    // 裸 exe 不像 Tauri 那样自带 DPI 声明，不先调这个，下面的坐标会是两个坐标系混着的
    podapp_win::ensure_dpi_aware();

    if std::env::args().any(|a| a == "--verify") {
        std::process::exit(verify());
    }
    // `--watch N`：跟随 N 秒，把每次位置变化打出来。拖动 Codex 窗口就能看见浮舱会贴到哪。
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--watch") {
        let secs: u64 = args
            .iter()
            .skip_while(|a| *a != "--watch")
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);
        println!("跟随 {secs} 秒 —— 现在去拖动 / 缩放 Codex 窗口试试。\n");
        let _w = podapp_win::watch(
            CODEX_APP,
            Box::new(|w| match w {
                Some(w) => println!(
                    "  位置变化 → x={} y={} w={} h={}  浮舱贴 x={}",
                    w.rect.x,
                    w.rect.y,
                    w.rect.w,
                    w.rect.h,
                    w.rect.right()
                ),
                None => println!("  宿主消失 —— 浮舱退到屏幕右缘独立停靠"),
            }),
        );
        std::thread::sleep(std::time::Duration::from_secs(secs));
        println!("\n跟随结束。");
        return;
    }

    let pids = pids_of(&CODEX_APP);
    println!("{} 进程：{} 个 {:?}", CODEX_APP.label, pids.len(), pids);

    match find_host_window(&CODEX_APP) {
        Some(w) => {
            println!("主窗口：hwnd={} pid={} 标题={:?}", w.hwnd, w.pid, w.title);
            println!(
                "位置：x={} y={} w={} h={}（右边缘 {}）",
                w.rect.x,
                w.rect.y,
                w.rect.w,
                w.rect.h,
                w.rect.right()
            );

            let work = podapp_win::work_area(Some(w.hwnd));
            println!(
                "所在屏工作区：x={} y={} w={} h={}",
                work.x, work.y, work.w, work.h
            );

            let p = podapp_win::dock::place(
                Some(w.rect),
                work,
                true,
                podapp_win::dock::Metrics::platform(),
            );
            let room = work.right() - w.rect.right();
            println!(
                "\n浮舱：x={} y={} w={} h={}（{:?}）",
                p.rect.x, p.rect.y, p.rect.w, p.rect.h, p.anchor
            );
            if p.rect.x < w.rect.right() {
                println!(
                    "宿主右边只剩 {room}px，放不下 {}px 的浮舱 —— 压在宿主右侧上方。",
                    p.rect.w
                );
            } else {
                println!("宿主右边有 {room}px，浮舱贴在旁边，不遮挡。");
            }
        }
        None if pids.is_empty() => {
            println!("\n没找到 {} 的进程 —— 它没开着。", CODEX_APP.label);
            println!("浮舱这时退到屏幕右缘独立停靠，这是正常路径，不是错误。");
        }
        None => {
            println!("\n进程在，但没有可见主窗口（可能最小化到托盘，或还在启动）。");
        }
    }
}
