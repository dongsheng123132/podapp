//! 找到宿主 AI 应用的窗口，并跟随它移动。浮舱的「贴在 Codex 右边」靠这个。
//!
//! ## 为什么按可执行文件路径匹配，而不是窗口标题或进程名
//!
//! 这是在这台机器上实测出来的，不是推测：
//!
//! - Codex App 是微软商店 MSIX 包（`OpenAI.Codex`），装在
//!   `C:\Program Files\WindowsApps\OpenAI.Codex_<版本>_x64__<hash>\`。
//! - 包里同时有 `Codex.exe` 和 `ChatGPT.exe`，而**真正跑 GUI 的进程是 `ChatGPT.exe`**。
//! - 它的窗口标题是 **"ChatGPT"**，不是 "Codex"。
//! - 它是 Chromium 多进程：实测 11 个 `ChatGPT.exe`，**只有 1 个有窗口**。
//!
//! 于是三种朴素做法各自会怎么坏：
//!
//! | 做法 | 坏在哪 |
//! |---|---|
//! | 匹配窗口标题 `"Codex"` | 找不到 —— 标题是 "ChatGPT"，而且会随会话名变、随语言变 |
//! | 匹配进程名 `ChatGPT.exe` | 认错 —— 用户另装的 ChatGPT 桌面版同名，会贴到错的窗口上 |
//! | 探测安装目录再比对 | 易碎 —— `WindowsApps` 默认 ACL 受限，且路径里带版本号，升级即失效 |
//!
//! 所以这里的判据是：**进程的完整路径里含 `openai.codex`**（包名，升级不变），
//! 且文件名在已知列表里。路径直接从运行中的进程拿，不去读那个受限目录。
//!
//! 追上游内部结构是有代价的（U-King 的 `codex.rs` 为此删过一整块 computer-use 探测）。
//! 这里只依赖两件很稳的事：MSIX 包名，以及「GUI 进程有窗口」。都变了才需要改这里。

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::*;

/// 屏幕坐标下的窗口矩形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// 明显不是「一个正常的主窗口」的尺寸。Chromium 会开一堆 1×1 的隐藏辅助窗口，
    /// 贴上去的话浮舱会飞到屏幕角落，而且看不出为什么。
    pub fn looks_like_a_real_window(&self) -> bool {
        self.w >= 200 && self.h >= 200
    }
}

/// 一个找到的宿主窗口。
#[derive(Debug, Clone)]
pub struct HostWindow {
    /// `HWND` 的整数形式。跨线程传它是安全的，句柄本身不拥有资源。
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    pub rect: Rect,
}

/// 一类可以被吸附的宿主应用。
#[derive(Debug, Clone, Copy)]
pub struct HostApp {
    /// 人话名字，用在日志和界面里
    pub label: &'static str,
    /// 进程完整路径里必须出现的片段（小写比对）。选**不随版本变**的那部分。
    pub path_marker: &'static str,
    /// 可执行文件名（小写）。多个是因为一个包里可能有多个入口。
    pub exe_names: &'static [&'static str],
}

/// Codex App（Windows 微软商店版）。
pub const CODEX_APP: HostApp = HostApp {
    label: "Codex App",
    // 包名 `OpenAI.Codex` 在安装路径里，升级只变后面的版本号
    path_marker: "openai.codex",
    // 实测 GUI 跑的是 ChatGPT.exe；Codex.exe 一并列上，免得上游哪天换了主入口
    exe_names: &["chatgpt.exe", "codex.exe"],
};

#[cfg(not(windows))]
mod stub {
    use super::*;

    /// 非 Windows 平台还没做。**明确返回「没找到」而不是假装成功** ——
    /// 浮舱在找不到宿主时本来就有独立停靠的退路，走那条路是对的。
    pub fn find_host_window(_app: &HostApp) -> Option<HostWindow> {
        None
    }
}

#[cfg(not(windows))]
pub use stub::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_helper_windows_are_not_mistaken_for_the_main_window() {
        // Chromium 会开一堆 1×1 的隐藏窗口；贴到那上面浮舱就飞了
        assert!(!Rect { x: 0, y: 0, w: 1, h: 1 }.looks_like_a_real_window());
        assert!(!Rect { x: 0, y: 0, w: 800, h: 12 }.looks_like_a_real_window());
        assert!(Rect { x: 100, y: 100, w: 1200, h: 800 }.looks_like_a_real_window());
    }

    #[test]
    fn rect_edges() {
        let r = Rect { x: 10, y: 20, w: 100, h: 50 };
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
    }
}
