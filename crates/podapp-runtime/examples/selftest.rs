//! 端到端自检：`cargo run --example selftest`
//!
//! 单元测试证明不了的东西在这里证明 —— 真的解包、真的原子换入、真的起 Node 子进程、
//! 真的验证沙箱拦得住越狱。宿主的 `--selftest` 开关也应该调到同一个 [`podapp_runtime::selftest::run`]，
//! 而不是自己再写一遍检查项。

fn main() {
    let fail = podapp_runtime::selftest::run();
    std::process::exit(if fail == 0 { 0 } else { 1 });
}
