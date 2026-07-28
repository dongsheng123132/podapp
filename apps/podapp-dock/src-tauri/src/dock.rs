//! 浮舱窗口状态与定位。
//!
//! 浮舱有两种用户可见状态：
//! - `Attached`：跟随宿主窗口；
//! - `Free`：用户自己摆放，只在拖动结束时做一次屏幕边缘磁吸。
//!
//! 宿主监听始终运行，但自由模式绝不因宿主移动而抢走用户选中的位置。

use podapp_win::{
    dock::{place, resize_at_snap, snap_to_work_area, Metrics, SnapEdge},
    HostWindow, Rect,
};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

pub const DOCK_LABEL: &str = "dock";
const FREE_EXPANDED_HEIGHT: i32 = 720;
const SNAP_THRESHOLD: i32 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockMode {
    Attached,
    Free,
}

#[derive(Clone)]
pub struct DockState {
    pub expanded: bool,
    pub host: Option<HostWindow>,
    pub mode: DockMode,
    pub free_rect: Option<Rect>,
    pub snap_edge: Option<SnapEdge>,
    /// 后台主动要求的位置。系统回报同一位置时不是用户拖动，必须忽略。
    pub expected_position: Option<(i32, i32)>,
    pub move_generation: u64,
    pub last_moved_position: Option<(i32, i32)>,
    pub user_drag_active: bool,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            expanded: false,
            host: None,
            mode: DockMode::Attached,
            free_rect: None,
            snap_edge: None,
            expected_position: None,
            move_generation: 0,
            last_moved_position: None,
            user_drag_active: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DockPlacement {
    pub placement: &'static str,
    pub snap_edge: Option<&'static str>,
    pub attached: bool,
    pub host_available: bool,
    pub host_title: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub static STATE: Mutex<Option<DockState>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut DockState) -> T) -> T {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(DockState::default))
}

fn free_size(expanded: bool, work: Rect) -> (i32, i32) {
    let metrics = Metrics::platform();
    let width = metrics.width(expanded).min(work.w.max(1));
    let height = if expanded {
        FREE_EXPANDED_HEIGHT.min(work.h).max(120.min(work.h.max(1)))
    } else {
        metrics.collapsed_h.min(work.h).max(1)
    };
    (width, height)
}

fn free_rect(state: &DockState) -> (Rect, Rect) {
    let fallback_work = podapp_win::work_area(None);
    let base = state.free_rect.unwrap_or(Rect {
        x: fallback_work.right() - Metrics::platform().width(state.expanded),
        y: fallback_work.y,
        w: Metrics::platform().width(state.expanded),
        h: if state.expanded {
            FREE_EXPANDED_HEIGHT.min(fallback_work.h)
        } else {
            Metrics::platform().collapsed_h
        },
    });
    let work = podapp_win::work_area_at(base.x + base.w / 2, base.y + base.h / 2);
    let (width, height) = free_size(state.expanded, work);
    (
        resize_at_snap(base, width, height, state.snap_edge, work),
        work,
    )
}

fn planned_rect(state: &DockState) -> Rect {
    match state.mode {
        DockMode::Attached => {
            let host_rect = state.host.as_ref().map(|h| h.rect);
            let host_hwnd = state.host.as_ref().map(|h| h.hwnd);
            place(
                host_rect,
                podapp_win::work_area(host_hwnd),
                state.expanded,
                Metrics::platform(),
            )
            .rect
        }
        DockMode::Free => free_rect(state).0,
    }
}

fn placement(state: &DockState, rect: Rect) -> DockPlacement {
    DockPlacement {
        placement: match state.mode {
            DockMode::Attached => "attached",
            DockMode::Free => "free",
        },
        snap_edge: state.snap_edge.map(SnapEdge::as_str),
        attached: state.mode == DockMode::Attached && state.host.is_some(),
        host_available: state.host.is_some(),
        host_title: state.host.as_ref().map(|h| h.title.clone()),
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
    }
}

/// 按当前模式摆放窗口。自由模式只应用已经保存的位置，不跟随宿主。
pub fn reposition(app: &AppHandle) -> Option<DockPlacement> {
    let win = app.get_webview_window(DOCK_LABEL)?;
    let state = with_state(|s| s.clone());
    let rect = planned_rect(&state);

    let want = PhysicalSize::new(rect.w.max(1) as u32, rect.h.max(1) as u32);
    let expected_generation = with_state(|s| {
        s.move_generation = s.move_generation.wrapping_add(1);
        s.expected_position = Some((rect.x, rect.y));
        s.move_generation
    });
    let _ = win.set_size(want);
    let _ = win.set_position(PhysicalPosition::new(rect.x, rect.y));
    let _ = win.set_always_on_top(true);

    if state.mode == DockMode::Free {
        with_state(|s| s.free_rect = Some(rect));
    }
    let result = placement(&state, rect);
    let _ = app.emit("dock://placed", &result);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        with_state(|s| {
            if s.move_generation == expected_generation {
                s.expected_position = None;
            }
        });
    });
    Some(result)
}

/// 展开/收起。自由模式保持已磁吸的边，吸附模式保持宿主锚点。
pub fn set_expanded(app: &AppHandle, on: bool) {
    with_state(|s| s.expanded = on);
    reposition(app);
}

pub fn is_expanded() -> bool {
    with_state(|s| s.expanded)
}

pub fn placement_summary() -> DockPlacement {
    let state = with_state(|s| s.clone());
    placement(&state, planned_rect(&state))
}

/// 用户拖动完成。位置只在这里磁吸一次，此后后台不再重算。
pub fn finish_drag(app: &AppHandle, x: i32, y: i32) -> DockPlacement {
    let state = with_state(|s| s.clone());
    let current = planned_rect(&state);
    let candidate = Rect {
        x,
        y,
        w: current.w,
        h: current.h,
    };
    let work =
        podapp_win::work_area_at(candidate.x + candidate.w / 2, candidate.y + candidate.h / 2);
    let (rect, edge) = snap_to_work_area(candidate, work, SNAP_THRESHOLD);
    with_state(|s| {
        s.mode = DockMode::Free;
        s.free_rect = Some(rect);
        s.snap_edge = edge;
        s.move_generation = s.move_generation.wrapping_add(1);
        s.last_moved_position = None;
        s.user_drag_active = false;
    });
    reposition(app).unwrap_or_else(|| placement(&with_state(|s| s.clone()), rect))
}

pub fn begin_drag() {
    with_state(|s| {
        s.user_drag_active = true;
        s.move_generation = s.move_generation.wrapping_add(1);
        s.last_moved_position = None;
    });
}

pub fn cancel_drag() {
    with_state(|s| {
        s.user_drag_active = false;
        s.move_generation = s.move_generation.wrapping_add(1);
        s.last_moved_position = None;
    });
}

/// 接收窗口系统回报的位置。后台主动 `set_position` 的回报会被排除；其余位置
/// 视为用户或系统窗口管理器的外部移动，停止 220ms 后进入自由模式并磁吸。
///
/// 这条后端兜底很重要：WebView 的原生拖动区域在某些 WebView2 版本会吞掉
/// `pointerdown`/`onMoved`，不能把用户位置是否被记住押在前端事件一定送达上。
pub fn note_window_moved(app: AppHandle, x: i32, y: i32) {
    let generation = with_state(|s| {
        if let Some((want_x, want_y)) = s.expected_position {
            if (want_x - x).abs() <= 2 && (want_y - y).abs() <= 2 {
                s.expected_position = None;
            }
            return None;
        }
        s.last_moved_position = Some((x, y));
        s.move_generation = s.move_generation.wrapping_add(1);
        if s.user_drag_active {
            return None;
        }
        Some(s.move_generation)
    });
    let Some(generation) = generation else {
        return;
    };

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(220));
        let final_position = with_state(|s| {
            (s.move_generation == generation)
                .then_some(s.last_moved_position)
                .flatten()
        });
        if let Some((x, y)) = final_position {
            finish_drag(&app, x, y);
        }
    });
}

/// 从前端持久化的位置恢复自由模式。
pub fn restore_free(app: &AppHandle, x: i32, y: i32, edge: Option<&str>) -> DockPlacement {
    let edge = edge.and_then(parse_edge);
    let state = with_state(|s| s.clone());
    let current = planned_rect(&state);
    let candidate = Rect {
        x,
        y,
        w: current.w,
        h: current.h,
    };
    let work =
        podapp_win::work_area_at(candidate.x + candidate.w / 2, candidate.y + candidate.h / 2);
    let (width, height) = free_size(state.expanded, work);
    let rect = resize_at_snap(candidate, width, height, edge, work);
    with_state(|s| {
        s.mode = DockMode::Free;
        s.free_rect = Some(rect);
        s.snap_edge = edge;
        s.move_generation = s.move_generation.wrapping_add(1);
        s.last_moved_position = None;
        s.user_drag_active = false;
    });
    reposition(app).unwrap_or_else(|| placement(&with_state(|s| s.clone()), rect))
}

pub fn attach(app: &AppHandle) -> DockPlacement {
    with_state(|s| {
        s.mode = DockMode::Attached;
        s.snap_edge = None;
        s.move_generation = s.move_generation.wrapping_add(1);
        s.last_moved_position = None;
        s.user_drag_active = false;
    });
    reposition(app).unwrap_or_else(placement_summary)
}

fn parse_edge(value: &str) -> Option<SnapEdge> {
    match value {
        "left" => Some(SnapEdge::Left),
        "right" => Some(SnapEdge::Right),
        "top" => Some(SnapEdge::Top),
        "bottom" => Some(SnapEdge::Bottom),
        "top-left" => Some(SnapEdge::TopLeft),
        "top-right" => Some(SnapEdge::TopRight),
        "bottom-left" => Some(SnapEdge::BottomLeft),
        "bottom-right" => Some(SnapEdge::BottomRight),
        _ => None,
    }
}

/// 当前浮舱所在显示器的可用区域。Pod 窗口沿用这套物理坐标。
pub fn current_work_area() -> Rect {
    let state = with_state(|s| s.clone());
    match state.mode {
        DockMode::Attached => podapp_win::work_area(state.host.as_ref().map(|h| h.hwnd)),
        DockMode::Free => {
            let rect = planned_rect(&state);
            podapp_win::work_area_at(rect.x + rect.w / 2, rect.y + rect.h / 2)
        }
    }
}

/// 当前状态最终应该落到的矩形。其他窗口需要同步锚定时认这份结果。
pub fn target_rect() -> Rect {
    planned_rect(&with_state(|s| s.clone()))
}

/// 开始监听宿主。自由模式仍记录宿主是否可用，但不移动浮舱。
pub fn start_following(app: AppHandle) -> podapp_win::Watcher {
    podapp_win::watch(
        podapp_win::CODEX_APP,
        Box::new(move |w| {
            let attached = with_state(|s| {
                s.host = w;
                s.mode == DockMode::Attached
            });
            if attached {
                reposition(&app);
            } else {
                let state = with_state(|s| s.clone());
                let _ = app.emit("dock://placed", placement(&state, planned_rect(&state)));
            }
        }),
    )
}
