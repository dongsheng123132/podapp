//! 浮舱窗口 —— 贴在宿主旁边、跟着它走。
//!
//! 位置怎么算不在这里（在 [`podapp_win::dock::place`]，纯函数、已穷举测过）。
//! 这里只干两件事：把算出来的位置**贴到真窗口上**，以及在宿主来去时切换状态。

use podapp_win::{dock::place, dock::Metrics, HostWindow, Rect};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

pub const DOCK_LABEL: &str = "dock";

/// 浮舱当前状态。展开与否是用户意图，宿主位置是外部事实，两者都要参与定位。
#[derive(Default)]
pub struct DockState {
    pub expanded: bool,
    pub host: Option<HostWindow>,
}

pub static STATE: Mutex<Option<DockState>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut DockState) -> T) -> T {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(DockState::default))
}

/// 把浮舱摆到它该在的位置。
///
/// 坐标一律 `Physical*`：[`podapp_win`] 给的就是物理像素（DWM + per-monitor DPI），
/// 用 `Logical*` 会在非 100% 缩放的屏幕上系统性偏移，而偏移量恰好像「差了个边框」。
pub fn reposition(app: &AppHandle) {
    let Some(win) = app.get_webview_window(DOCK_LABEL) else { return };
    let (host_rect, expanded) = with_state(|s| (s.host.as_ref().map(|h| h.rect), s.expanded));

    let work = podapp_win::work_area(with_state(|s| s.host.as_ref().map(|h| h.hwnd)));
    // 平台下限从这里注入 —— 几何那边保持纯函数
    let p = place(host_rect, work, expanded, Metrics::platform());

    // 宽度已经把平台下限算进去了（Metrics::platform），所以这里请求什么就得到什么。
    // 曾经试过 set_min_size(1,1) 让 tao 接管 WM_GETMINMAXINFO 来要一个比 SM_CXMIN
    // 更窄的窗口 —— 不管用：事后 GetWindowRect 和 DWM 双双报 170，
    // 只有 tao 自己的 outer_size() 回 64（它回的是请求值不是实际值）。
    // 所以改成顺着平台走，让「算出来的」和「实际的」永远一致。
    let want = PhysicalSize::new(p.rect.w.max(1) as u32, p.rect.h.max(1) as u32);
    let _ = win.set_size(want);
    let _ = win.set_position(PhysicalPosition::new(p.rect.x, p.rect.y));
    // 置顶会被别的置顶窗口抢走，每次移动重新声明一次，代价极低
    let _ = win.set_always_on_top(true);

    let _ = app.emit(
        "dock://placed",
        serde_json::json!({
            "anchor": format!("{:?}", p.anchor),
            "attached": host_rect.is_some(),
            "hostTitle": with_state(|s| s.host.as_ref().map(|h| h.title.clone())),
            "rect": { "x": p.rect.x, "y": p.rect.y, "w": p.rect.w, "h": p.rect.h },
        }),
    );
}

/// 展开 / 收起。只改宽度，不换边（见 `place` 的测试）。
pub fn set_expanded(app: &AppHandle, on: bool) {
    with_state(|s| s.expanded = on);
    reposition(app);
}

pub fn is_expanded() -> bool {
    with_state(|s| s.expanded)
}

/// 当前贴着谁。给前端显示「已吸附到 Codex」用。
pub fn host_summary() -> Option<(String, Rect)> {
    with_state(|s| s.host.as_ref().map(|h| (h.title.clone(), h.rect)))
}

/// 开始跟随宿主。返回的 watcher 要一直活着 —— drop 了就停了。
pub fn start_following(app: AppHandle) -> podapp_win::Watcher {
    podapp_win::watch(
        podapp_win::CODEX_APP,
        Box::new(move |w| {
            with_state(|s| s.host = w);
            reposition(&app);
        }),
    )
}
