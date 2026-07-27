//! 探针：`cargo run -p podapp-win --example probe`
//!
//! 在真机上回答一个问题：**现在能不能找到 Codex 的主窗口，它在哪。**
//! 浮舱吸附对不对，最终只能这样验 —— 单元测试证明不了「屏幕上那个窗口是不是它」。

use podapp_win::{find_host_window, pids_of, CODEX_APP};

fn main() {
    let pids = pids_of(&CODEX_APP);
    println!("{} 进程：{} 个 {:?}", CODEX_APP.label, pids.len(), pids);

    match find_host_window(&CODEX_APP) {
        Some(w) => {
            println!("主窗口：hwnd={} pid={} 标题={:?}", w.hwnd, w.pid, w.title);
            println!(
                "位置：x={} y={} w={} h={}（右边缘 {}）",
                w.rect.x, w.rect.y, w.rect.w, w.rect.h, w.rect.right()
            );
            println!("\n浮舱会贴到 x={} 处，高度跟随 {}。", w.rect.right(), w.rect.h);
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
