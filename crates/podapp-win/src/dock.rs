//! 浮舱该停在哪 —— 纯几何，不碰窗口系统。
//!
//! 单独拎出来是因为这是**唯一会算错的地方**，而算错的表现（浮舱飞到屏幕外、
//! 盖住宿主、贴反了边）在真机上排查一次要好几分钟。纯函数就能穷举掉：
//! 宿主贴着右边缘、宿主比屏幕还宽、宿主在副屏、宿主没开着 —— 全是几行断言的事。

use crate::Rect;

/// 浮舱宽度（物理像素）。收起时只露出小船。
pub const DOCK_WIDTH_EXPANDED: i32 = 380;
pub const DOCK_WIDTH_COLLAPSED: i32 = 64;
pub const DOCK_HEIGHT_COLLAPSED: i32 = 64;

/// 贴在宿主右侧时留的缝。0 表示严丝合缝。
const GAP: i32 = 0;

/// 浮舱的尺寸参数，外加**平台强加的下限**。
///
/// 把下限做成入参而不是在 [`place`] 里直接查系统：几何保持纯函数，
/// 才能把宿主占满屏、被拖出屏幕这些畸形输入用几行断言穷举掉。
/// 平台那部分由调用方注入（见 [`crate::min_window_width`]）。
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub expanded_w: i32,
    pub collapsed_w: i32,
    pub collapsed_h: i32,
    /// 系统允许的最小窗口宽度。0 = 不设限（测试和非 Windows 用）。
    pub min_w: i32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            expanded_w: DOCK_WIDTH_EXPANDED,
            collapsed_w: DOCK_WIDTH_COLLAPSED,
            collapsed_h: DOCK_HEIGHT_COLLAPSED,
            min_w: 0,
        }
    }
}

impl Metrics {
    /// 带上本机平台下限的一份参数。
    #[cfg(windows)]
    pub fn platform() -> Self {
        Self {
            min_w: crate::min_window_width(),
            ..Default::default()
        }
    }

    pub fn width(&self, expanded: bool) -> i32 {
        let w = if expanded {
            self.expanded_w
        } else {
            self.collapsed_w
        };
        w.max(self.min_w)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// 贴在宿主窗口右侧
    HostRight,
    /// 宿主不在，退到工作区右缘独立停靠
    ScreenRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub rect: Rect,
    pub anchor: Anchor,
}

/// 自由漂浮时吸附到工作区的哪一边。
///
/// 角是独立状态，而不是让调用方同时保存两条边。这样持久化格式稳定，
/// 展开/收起改变尺寸时也不会丢掉其中一条边。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl SnapEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }
}

fn clamp_axis(value: i32, start: i32, extent: i32, size: i32) -> i32 {
    if size >= extent {
        start
    } else {
        value.clamp(start, start + extent - size)
    }
}

/// 将用户拖到的新位置限制在工作区内，并在靠近边缘时产生磁吸。
///
/// `threshold` 是物理像素。传 0 仍会把已经越界的窗口吸回最近边缘，
/// 因而任何调用都不会留下一个够不着的窗口。
pub fn snap_to_work_area(rect: Rect, work: Rect, threshold: i32) -> (Rect, Option<SnapEdge>) {
    let w = rect.w.clamp(1, work.w.max(1));
    let h = rect.h.clamp(1, work.h.max(1));
    let right = rect.x.saturating_add(w);
    let bottom = rect.y.saturating_add(h);
    let threshold = threshold.max(0);

    let near_left = rect.x <= work.x || (rect.x - work.x).abs() <= threshold;
    let near_right =
        right >= work.right() || (work.right().saturating_sub(right)).abs() <= threshold;
    let near_top = rect.y <= work.y || (rect.y - work.y).abs() <= threshold;
    let near_bottom =
        bottom >= work.bottom() || (work.bottom().saturating_sub(bottom)).abs() <= threshold;

    let horizontal = match (near_left, near_right) {
        (true, true) => {
            let left_gap = (rect.x - work.x).abs();
            let right_gap = (work.right().saturating_sub(right)).abs();
            Some(if left_gap <= right_gap {
                SnapEdge::Left
            } else {
                SnapEdge::Right
            })
        }
        (true, false) => Some(SnapEdge::Left),
        (false, true) => Some(SnapEdge::Right),
        (false, false) => None,
    };
    let vertical = match (near_top, near_bottom) {
        (true, true) => {
            let top_gap = (rect.y - work.y).abs();
            let bottom_gap = (work.bottom().saturating_sub(bottom)).abs();
            Some(if top_gap <= bottom_gap {
                SnapEdge::Top
            } else {
                SnapEdge::Bottom
            })
        }
        (true, false) => Some(SnapEdge::Top),
        (false, true) => Some(SnapEdge::Bottom),
        (false, false) => None,
    };

    let edge = match (horizontal, vertical) {
        (Some(SnapEdge::Left), Some(SnapEdge::Top)) => Some(SnapEdge::TopLeft),
        (Some(SnapEdge::Right), Some(SnapEdge::Top)) => Some(SnapEdge::TopRight),
        (Some(SnapEdge::Left), Some(SnapEdge::Bottom)) => Some(SnapEdge::BottomLeft),
        (Some(SnapEdge::Right), Some(SnapEdge::Bottom)) => Some(SnapEdge::BottomRight),
        (Some(edge), None) | (None, Some(edge)) => Some(edge),
        _ => None,
    };

    (resize_at_snap(rect, w, h, edge, work), edge)
}

/// 改变自由窗口尺寸，同时保持已吸附的边或角不动。
pub fn resize_at_snap(
    rect: Rect,
    width: i32,
    height: i32,
    edge: Option<SnapEdge>,
    work: Rect,
) -> Rect {
    let w = width.clamp(1, work.w.max(1));
    let h = height.clamp(1, work.h.max(1));
    let mut x = clamp_axis(rect.x, work.x, work.w, w);
    let mut y = clamp_axis(rect.y, work.y, work.h, h);

    match edge {
        Some(SnapEdge::Left | SnapEdge::TopLeft | SnapEdge::BottomLeft) => x = work.x,
        Some(SnapEdge::Right | SnapEdge::TopRight | SnapEdge::BottomRight) => x = work.right() - w,
        _ => {}
    }
    match edge {
        Some(SnapEdge::Top | SnapEdge::TopLeft | SnapEdge::TopRight) => y = work.y,
        Some(SnapEdge::Bottom | SnapEdge::BottomLeft | SnapEdge::BottomRight) => {
            y = work.bottom() - h
        }
        _ => {}
    }

    Rect { x, y, w, h }
}

/// 算浮舱应该在哪。
///
/// **坐标一律是物理像素。** `Rect` 来自 DWM extended frame bounds，本来就是物理的；
/// Tauri 那边必须配 `PhysicalPosition`/`PhysicalSize`。混进逻辑像素的话，
/// 在非 100% 缩放的屏幕上会偏，而偏移量恰好像「差了个边框」，很容易查错方向。
///
/// - `host`：宿主窗口矩形，`None` = 宿主没开着（**常态，不是错误**）
/// - `work`：屏幕工作区（已去掉任务栏）
pub fn place(host: Option<Rect>, work: Rect, expanded: bool, m: Metrics) -> Placement {
    let w = m.width(expanded);

    let (mut x, y, h, anchor) = match host {
        Some(hr) => (hr.right() + GAP, hr.y, hr.h, Anchor::HostRight),
        None => (work.right() - w, work.y, work.h, Anchor::ScreenRight),
    };

    // 宿主占满屏幕时右侧没地方了 —— 改贴到宿主右边缘**内侧**，压在它上面。
    // 这比让浮舱跑到屏幕外好：看不见的窗口对用户等于程序坏了。
    if x + w > work.right() {
        x = work.right() - w;
    }
    // 副屏在左、或宿主被拖到屏幕左外侧时，别把浮舱推到更左
    if x < work.x {
        x = work.x;
    }

    // 展开后高度跟随宿主；收起时必须真的是一个按钮，不能留一整条透明置顶窗口。
    let y = y.max(work.y);
    let available_h = work.bottom() - y;
    let h = if expanded {
        h.min(available_h).max(120)
    } else {
        m.collapsed_h.min(available_h).max(1)
    };

    Placement {
        rect: Rect { x, y, w, h },
        anchor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 2560,
            h: 1400,
        }
    }

    #[test]
    fn snaps_beside_the_host_when_there_is_room() {
        // 宿主右边留了 500px，够放下 380 的浮舱 —— 这时不该压在宿主上
        let host = Rect {
            x: 200,
            y: 250,
            w: 1800,
            h: 1025,
        };
        let p = place(Some(host), work(), true, Metrics::default());
        assert_eq!(p.anchor, Anchor::HostRight);
        assert_eq!(p.rect.x, 2000, "该紧贴宿主右边缘");
        assert_eq!(p.rect.y, 250, "顶部跟宿主对齐");
        assert_eq!(p.rect.h, 1025, "高度跟随宿主");
        assert_eq!(p.rect.w, DOCK_WIDTH_EXPANDED);
    }

    #[test]
    fn overlaps_the_host_when_there_is_no_room_beside_it() {
        // 真机实测的那组数：Codex 在 x=999 w=1508，右边缘 2507，屏幕才 2560 宽 ——
        // 右边只剩 53px，放不下 380 的浮舱。
        //
        // 这**不是**边缘情况，而是常态：单屏用户的 Codex 多半是最大化或占大半屏。
        // 所以这里的选择是压在宿主右侧上方（像 Raycast 那类浮层），
        // 而不是把浮舱推出屏幕 —— 看不见的窗口对用户等于程序坏了。
        let host = Rect {
            x: 999,
            y: 250,
            w: 1508,
            h: 1025,
        };
        let p = place(Some(host), work(), true, Metrics::default());
        assert_eq!(p.rect.right(), work().right(), "贴住屏幕右缘");
        assert!(p.rect.x < host.right(), "确实压在宿主上");
        assert_eq!(p.rect.y, 250, "仍然跟宿主上下对齐，不是贴满全屏");
        assert_eq!(p.rect.h, 1025);
    }

    #[test]
    fn falls_back_to_screen_edge_when_the_host_is_not_running() {
        // 宿主没开着是常态：浮舱退到工作区右缘，而不是消失或报错
        let p = place(None, work(), false, Metrics::default());
        assert_eq!(p.anchor, Anchor::ScreenRight);
        assert_eq!(p.rect.right(), work().right());
        assert_eq!(p.rect.w, DOCK_WIDTH_COLLAPSED);
        assert_eq!(p.rect.h, DOCK_HEIGHT_COLLAPSED);
    }

    #[test]
    fn collapsed_dock_is_button_height_even_when_the_host_is_tall() {
        let host = Rect {
            x: 200,
            y: 250,
            w: 1800,
            h: 1025,
        };
        let p = place(Some(host), work(), false, Metrics::default());
        assert_eq!(p.rect.y, host.y, "收起和展开共用顶部锚点");
        assert_eq!(p.rect.h, DOCK_HEIGHT_COLLAPSED, "收起态不能留一整条黑栏");
    }

    #[test]
    fn never_lands_off_screen_even_if_the_host_fills_it() {
        // 宿主最大化 / 比屏幕还宽时，右边没地方了 —— 压在宿主上，而不是飞到屏幕外
        for host in [
            Rect {
                x: 0,
                y: 0,
                w: 2560,
                h: 1400,
            }, // 正好占满
            Rect {
                x: 0,
                y: 0,
                w: 4000,
                h: 1400,
            }, // 比屏幕还宽
            Rect {
                x: 2400,
                y: 0,
                w: 1000,
                h: 1400,
            }, // 大半在屏幕外
        ] {
            let p = place(Some(host), work(), true, Metrics::default());
            assert!(p.rect.x >= work().x, "跑到屏幕左外了: {p:?}");
            assert!(p.rect.right() <= work().right(), "跑到屏幕右外了: {p:?}");
        }
    }

    #[test]
    fn a_host_above_the_work_area_does_not_drag_the_dock_up() {
        // 宿主标题栏被拖到屏幕上方之外时，浮舱顶部仍应留在工作区内
        let host = Rect {
            x: 100,
            y: -300,
            w: 800,
            h: 900,
        };
        let p = place(Some(host), work(), true, Metrics::default());
        assert!(p.rect.y >= work().y, "浮舱顶部跑到工作区上方了: {p:?}");
        assert!(
            p.rect.bottom() <= work().bottom(),
            "浮舱底部超出工作区了: {p:?}"
        );
    }

    #[test]
    fn a_very_short_host_still_leaves_a_usable_dock() {
        let host = Rect {
            x: 100,
            y: 100,
            w: 800,
            h: 20,
        };
        let p = place(Some(host), work(), true, Metrics::default());
        assert!(p.rect.h >= 120, "浮舱被挤成一条了: {p:?}");
    }

    #[test]
    fn the_platform_floor_wins_over_the_desired_width() {
        // Windows 把顶层窗口卡在 SM_CXMIN（实测 170px），而收起态只想要 64px。
        // 试过 set_min_size(1,1) 绕开，不行 —— GetWindowRect 和 DWM 双双报 170。
        //
        // 所以不跟系统较劲，把下限算进来。**关键是「算出来的」要等于「实际的」**：
        // 假装自己是 64 而系统给 170，会让停靠位置差 106px，而没有任何报错。
        let m = Metrics {
            min_w: 170,
            ..Default::default()
        };
        let host = Rect {
            x: 200,
            y: 250,
            w: 1800,
            h: 1025,
        };

        let c = place(Some(host), work(), false, m);
        assert_eq!(c.rect.w, 170, "收起宽度该被抬到平台下限");

        // 展开宽度本来就大于下限，不该被影响
        let e = place(Some(host), work(), true, m);
        assert_eq!(e.rect.w, DOCK_WIDTH_EXPANDED);

        // 没有平台下限时（测试 / 非 Windows）仍是原来的值
        let c0 = place(Some(host), work(), false, Metrics::default());
        assert_eq!(c0.rect.w, DOCK_WIDTH_COLLAPSED);
    }

    #[test]
    fn collapsing_grows_from_a_stable_edge() {
        // 收起/展开只该改宽度，不该让浮舱整个跳到别处。
        // 「哪条边不动」取决于有没有被屏幕挤住，两种情况都要成立：
        let host_roomy = Rect {
            x: 200,
            y: 250,
            w: 1800,
            h: 1025,
        };
        let e = place(Some(host_roomy), work(), true, Metrics::default());
        let c = place(Some(host_roomy), work(), false, Metrics::default());
        assert_eq!(e.rect.x, c.rect.x, "有地方时贴着宿主，左边缘不动、向右长");
        assert_eq!(e.anchor, c.anchor);
        assert!(c.rect.w < e.rect.w);

        let host_tight = Rect {
            x: 999,
            y: 250,
            w: 1508,
            h: 1025,
        };
        let e = place(Some(host_tight), work(), true, Metrics::default());
        let c = place(Some(host_tight), work(), false, Metrics::default());
        assert_eq!(
            e.rect.right(),
            c.rect.right(),
            "被挤住时贴着屏幕右缘，右边缘不动、向左长"
        );
        assert_eq!(e.rect.y, c.rect.y, "上下位置任何时候都不该跳");
    }

    #[test]
    fn free_window_snaps_to_all_sides_and_corners() {
        let cases = [
            (
                Rect {
                    x: 10,
                    y: 500,
                    w: 300,
                    h: 200,
                },
                SnapEdge::Left,
            ),
            (
                Rect {
                    x: 2255,
                    y: 500,
                    w: 300,
                    h: 200,
                },
                SnapEdge::Right,
            ),
            (
                Rect {
                    x: 500,
                    y: 8,
                    w: 300,
                    h: 200,
                },
                SnapEdge::Top,
            ),
            (
                Rect {
                    x: 500,
                    y: 1194,
                    w: 300,
                    h: 200,
                },
                SnapEdge::Bottom,
            ),
            (
                Rect {
                    x: 8,
                    y: 8,
                    w: 300,
                    h: 200,
                },
                SnapEdge::TopLeft,
            ),
            (
                Rect {
                    x: 2255,
                    y: 8,
                    w: 300,
                    h: 200,
                },
                SnapEdge::TopRight,
            ),
            (
                Rect {
                    x: 8,
                    y: 1194,
                    w: 300,
                    h: 200,
                },
                SnapEdge::BottomLeft,
            ),
            (
                Rect {
                    x: 2255,
                    y: 1194,
                    w: 300,
                    h: 200,
                },
                SnapEdge::BottomRight,
            ),
        ];

        for (rect, want) in cases {
            let (got, edge) = snap_to_work_area(rect, work(), 24);
            assert_eq!(edge, Some(want), "{rect:?}");
            assert!(got.x >= work().x && got.right() <= work().right());
            assert!(got.y >= work().y && got.bottom() <= work().bottom());
        }
    }

    #[test]
    fn free_window_can_stay_in_the_middle() {
        let rect = Rect {
            x: 600,
            y: 420,
            w: 300,
            h: 200,
        };
        assert_eq!(snap_to_work_area(rect, work(), 24), (rect, None));
    }

    #[test]
    fn resizing_keeps_the_selected_corner_fixed() {
        let small = Rect {
            x: 2390,
            y: 1336,
            w: 170,
            h: 64,
        };
        let large = resize_at_snap(
            small,
            DOCK_WIDTH_EXPANDED,
            720,
            Some(SnapEdge::BottomRight),
            work(),
        );
        assert_eq!(large.right(), work().right());
        assert_eq!(large.bottom(), work().bottom());
        assert_eq!(large.w, DOCK_WIDTH_EXPANDED);
        assert_eq!(large.h, 720);
    }

    #[test]
    fn dragging_outside_is_clamped_back_into_view() {
        let (rect, edge) = snap_to_work_area(
            Rect {
                x: -900,
                y: 1700,
                w: 300,
                h: 200,
            },
            work(),
            24,
        );
        assert_eq!(rect.x, work().x);
        assert_eq!(rect.bottom(), work().bottom());
        assert_eq!(edge, Some(SnapEdge::BottomLeft));
    }
}
