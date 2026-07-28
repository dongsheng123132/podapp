//! Windows 实现。

use crate::{HostApp, HostWindow, Rect};
use std::ffi::c_void;
use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsWindowVisible, SendMessageTimeoutW, GW_OWNER, SMTO_ABORTIFHUNG, WM_GETTEXT,
};

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    std::ffi::OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned()
}

/// 一个进程的完整可执行文件路径。
///
/// 拿不到就是拿不到（权限不够、进程刚退出），返回 `None` 让调用方跳过它 ——
/// 这在正常运行时每次枚举都会发生几次，不是异常。
fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 32768];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| wide_to_string(&buf[..len as usize]))
    }
}

/// 属于这个宿主应用的所有进程 ID。
///
/// 两道判据都要过：文件名在列表里，**且**完整路径含包名标记。
/// 只看文件名会把用户另装的同名应用认成宿主；只看路径则要枚举每个进程的完整路径，
/// 而那是个需要开句柄的系统调用 —— 先用文件名把候选筛到个位数再去开句柄。
pub fn pids_of(app: &HostApp) -> Vec<u32> {
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let name = wide_to_string(&e.szExeFile).to_ascii_lowercase();
                if app.exe_names.contains(&name.as_str()) {
                    if let Some(p) = process_path(e.th32ProcessID) {
                        if p.to_ascii_lowercase().contains(app.path_marker) {
                            out.push(e.th32ProcessID);
                        }
                    }
                }
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// 窗口的可视矩形。
///
/// 优先用 DWM 的 extended frame bounds：Win10 起 `GetWindowRect` 返回的矩形**包含
/// 左右各约 7px 的不可见拖拽边框**，直接拿它贴边会留下一条看得见的缝，
/// 而缝的宽度还随 DPI 变 —— 查起来会以为是自己算错了。DWM 拿不到时退回 GetWindowRect。
fn window_rect(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut r = std::mem::zeroed::<windows_sys::Win32::Foundation::RECT>();
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut r as *mut _ as *mut c_void,
            std::mem::size_of::<windows_sys::Win32::Foundation::RECT>() as u32,
        );
        if hr != 0 && GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        Some(Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        })
    }
}

/// 窗口标题。**永远不许阻塞**，拿不到就返回空串。
///
/// 不用 `GetWindowTextW`：对**同进程**的窗口它会同步发 `WM_GETTEXT`，一直等到那个窗口
/// 所在线程去抽消息为止。跟随线程要是卡在这儿，宿主一忙浮舱就整个僵住，
/// 而表现只是「浮舱不动了」，根本看不出是卡在读标题。
/// （这条是 `tests/follow.rs` 实测撞出来的死锁：跟随线程等测试线程抽消息，
/// 测试线程正等跟随线程回调。）
///
/// `SendMessageTimeoutW` + `SMTO_ABORTIFHUNG` 是唯一保证不会挂住的取法。
/// 标题只是给日志和界面看的装饰，为它冒卡死的风险不划算。
fn window_title(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let mut got: usize = 0;
        let ok = SendMessageTimeoutW(
            hwnd,
            WM_GETTEXT,
            buf.len(),
            buf.as_mut_ptr() as isize,
            SMTO_ABORTIFHUNG,
            200,
            &mut got,
        );
        if ok == 0 {
            return String::new();
        }
        wide_to_string(&buf)
    }
}

struct Scan {
    pids: Vec<u32>,
    /// 要不要用「像个主窗口」的尺寸启发式筛。找宿主时要，找自己时不要。
    require_main_size: bool,
    found: Vec<HostWindow>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
    let scan = &mut *(lparam as *mut Scan);

    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    // 有 owner 的是对话框/工具窗，不是主窗口
    if !GetWindow(hwnd, GW_OWNER).is_null() {
        return 1;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if !scan.pids.contains(&pid) {
        return 1;
    }
    let Some(rect) = window_rect(hwnd) else {
        return 1;
    };
    if scan.require_main_size && !rect.looks_like_a_real_window() {
        return 1;
    }
    scan.found.push(HostWindow {
        hwnd: hwnd as isize,
        pid,
        title: window_title(hwnd),
        rect,
    });
    1
}

fn scan_windows(app: &HostApp, require_main_size: bool) -> Vec<HostWindow> {
    let pids = pids_of(app);
    if pids.is_empty() {
        return vec![];
    }
    let mut scan = Scan {
        pids,
        require_main_size,
        found: vec![],
    };
    unsafe {
        EnumWindows(Some(enum_proc), &mut scan as *mut Scan as LPARAM);
    }
    scan.found
}

/// 找宿主应用的主窗口。找不到返回 `None` —— 宿主没开着是**常态**，不是错误。
///
/// 多个候选时挑面积最大的那个：Chromium 系应用除了主窗口还会有若干辅助顶层窗口，
/// 主窗口总是最大的那个。
pub fn find_host_window(app: &HostApp) -> Option<HostWindow> {
    scan_windows(app, true)
        .into_iter()
        .max_by_key(|w| (w.rect.w as i64) * (w.rect.h as i64))
}

/// 一个应用的**全部**可见顶层窗口，不做尺寸筛选。
///
/// [`find_host_window`] 那条尺寸启发式（≥200px）是为了绕开 Chromium 的 1×1 隐藏窗口，
/// 对「找别人家的主窗口」是对的。但浮舱自己收起时只有 64px 宽 —— 拿同一把尺子量它，
/// 会得出「浮舱没在跑」。所以要找已知形态的自家窗口时用这个。
pub fn find_windows_of(app: &HostApp) -> Vec<HostWindow> {
    scan_windows(app, false)
}

/// 这个窗口还在不在、现在在哪。跟随时用它取新位置，比整轮重扫便宜得多。
pub fn refresh(w: &HostWindow) -> Option<Rect> {
    let hwnd = w.hwnd as HWND;
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return None;
        }
    }
    window_rect(hwnd).filter(|r| r.looks_like_a_real_window())
}

/// 声明本进程「按显示器感知 DPI」。**必须在任何取坐标的调用之前跑一次。**
///
/// 不声明的话，Windows 会给进程一套**虚拟化**的坐标：`GetMonitorInfoW` 报逻辑像素，
/// 而 `DwmGetWindowAttribute` 报物理像素 —— 两个坐标系混在一起，算出来的位置似是而非。
///
/// 这不是假想。实测这台 1.25 倍缩放的机器上：工作区报 `w=2048`（= 2560 / 1.25，逻辑），
/// 宿主窗口右边缘报 `2507`（物理）。于是「宿主右边还剩多少地方」算出来是 **-459px**，
/// 一个物理上不可能的数。而它推出的结论（放不下，得压上去）**恰好是对的**，
/// 所以不盯着中间值看根本发现不了 —— 这种错最贵。
///
/// Tauri 的应用清单默认已经声明了 per-monitor v2，所以浮舱本体不受影响；
/// 但裸 exe（examples/probe、集成测试）不会，必须自己调一次。
///
/// 重复调用、或宿主已经声明过，都会失败并返回 `false`，那是正常的，不用管。
pub fn ensure_dpi_aware() -> bool {
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 }
}

/// 系统允许的最小顶层窗口宽度（`SM_CXMIN`，物理像素）。
///
/// Windows 会把顶层窗口卡在这个宽度，**而且是静默的**：`set_size(64)` 不报错、
/// 不警告，窗口就是比你要的宽。实测这台 1.25 倍缩放的机器上是 **170px**。
///
/// 试过用 `set_min_size(1,1)` 让 tao 接管 `WM_GETMINMAXINFO` 来绕开它 —— 不行：
/// 事后 `GetWindowRect` 和 DWM 双双报 170，只有 tao 自己的 `outer_size()` 说 64
///（它回的是请求值不是实际值，这一点本身就够坑）。
///
/// 所以不跟系统较劲，改成**把它当已知下限**参与计算。这样「算出来的位置」和
/// 「窗口实际在哪」永远一致 —— 那条一致性正是 `probe --verify` 在守的东西。
pub fn min_window_width() -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXMIN};
    unsafe { GetSystemMetrics(SM_CXMIN) }
}

/// 本进程的坐标是不是统一的物理像素。
///
/// 宿主启动时该查一次：`false` 表示还没声明 DPI 感知，此后拿到的所有坐标都可能
/// 是两个坐标系混着的。给它一个显式的查询函数，是因为这个错**不会以崩溃或异常值现身** ——
/// 它只是让位置算得似是而非，而中间值里那个负数没人会去看。
pub fn dpi_awareness_ok() -> bool {
    use windows_sys::Win32::UI::HiDpi::{
        GetAwarenessFromDpiAwarenessContext, GetThreadDpiAwarenessContext,
        DPI_AWARENESS_PER_MONITOR_AWARE,
    };
    unsafe {
        GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext())
            == DPI_AWARENESS_PER_MONITOR_AWARE
    }
}

/// 工作区（去掉任务栏之后能放窗口的那块）。
///
/// **按宿主窗口所在的那块屏取，不是主屏。** 用户把 Codex 拖到副屏上是常见的
/// —— 拿主屏的工作区去算，浮舱会停在另一块屏幕上，而用户看到的是「浮舱不见了」。
///
/// `near` 传宿主窗口的 hwnd；宿主没开着时传 `None`，取主屏。
pub fn work_area(near: Option<isize>) -> Rect {
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
        MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
    };
    unsafe {
        let mon = match near {
            Some(h) => MonitorFromWindow(h as HWND, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromPoint(
                windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
                MONITOR_DEFAULTTOPRIMARY,
            ),
        };
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            // 拿不到就给一个保守的默认，别让调用方拿到 0×0 去算除法
            return Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            };
        }
        let r = mi.rcWork;
        Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        }
    }
}

/// 包含这个屏幕坐标的显示器工作区；点落在所有屏幕外时取最近的一块。
///
/// 自由漂浮窗口没有宿主 hwnd 可借，因此必须按窗口中心点选屏。仍然使用物理像素，
/// 与 [`work_area`]、DWM 窗口矩形和 Tauri `PhysicalPosition` 保持同一坐标系。
pub fn work_area_at(x: i32, y: i32) -> Rect {
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    unsafe {
        let mon = MonitorFromPoint(
            windows_sys::Win32::Foundation::POINT { x, y },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            };
        }
        let r = mi.rcWork;
        Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        }
    }
}

// ───────────────────────────── 跟随 ─────────────────────────────

/// 跟随宿主窗口移动。
///
/// **移动走事件，发现走慢轮询** —— 两件事的要求完全不同，用同一种手段必然一边浪费一边卡：
///
/// - 宿主**移动**必须立刻跟上。轮询位置就算 30ms 一次，拖动窗口时浮舱也会明显滞后、
///   像被拖着的橡皮筋。所以挂 `SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE)`，
///   而且只挂在宿主那一个进程上（全局挂会收到整个桌面每个窗口的移动，白烧 CPU）。
/// - 宿主**有没有启动**只能问，因为没进程就没东西可挂钩子。但「Codex 刚开」晚一秒发现
///   完全无所谓，所以 1 秒一次，代价可以忽略。
///
/// 回调在专用线程上跑，不要在里面做重活。宿主没开着时回调收到 `None`，这是正常状态。
pub struct Watcher {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type ChangeFn = Box<dyn Fn(Option<HostWindow>) + Send>;

thread_local! {
    /// 钩子回调拿不到用户数据，只能走线程局部。钩子就设在这个线程上，
    /// 回调也投递到这个线程，所以线程局部是安全且够用的。
    static WATCH_TARGET: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    static WATCH_DIRTY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

unsafe extern "system" fn win_event_proc(
    _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // OBJID_WINDOW = 0：只关心窗口自己的移动，不关心它内部控件的
    if id_object != 0 {
        return;
    }
    WATCH_TARGET.with(|t| {
        if t.get() == hwnd as isize {
            WATCH_DIRTY.with(|d| d.set(true));
        }
    });
}

/// 开始跟随。返回的 [`Watcher`] 一旦 drop 就停。
pub fn watch(app: HostApp, on_change: ChangeFn) -> Watcher {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, EVENT_OBJECT_LOCATIONCHANGE,
        MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WINEVENT_OUTOFCONTEXT,
    };

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();

    let thread = std::thread::spawn(move || unsafe {
        let mut current: Option<HostWindow> = None;
        let mut hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK = std::ptr::null_mut();
        let mut hooked_pid = 0u32;

        while !stop2.load(Ordering::Relaxed) {
            // ① 慢节拍：宿主起没起、窗口还在不在
            let still_there = current.as_ref().and_then(refresh);
            match (&mut current, still_there) {
                // 还在，位置可能变了 —— 位置变化由钩子那边标脏，这里只管存活
                (Some(c), Some(r)) => {
                    if c.rect != r {
                        c.rect = r;
                        on_change(Some(c.clone()));
                    }
                }
                // 窗口没了：撤钩子，回到寻找状态
                (slot @ Some(_), None) => {
                    *slot = None;
                    if !hook.is_null() {
                        UnhookWinEvent(hook);
                        hook = std::ptr::null_mut();
                        hooked_pid = 0;
                    }
                    WATCH_TARGET.with(|t| t.set(0));
                    on_change(None);
                }
                // 还没找到：找一次
                (slot @ None, _) => {
                    if let Some(w) = find_host_window(&app) {
                        WATCH_TARGET.with(|t| t.set(w.hwnd));
                        if hooked_pid != w.pid {
                            if !hook.is_null() {
                                UnhookWinEvent(hook);
                            }
                            // 只挂宿主这一个进程：全局挂会收到整个桌面的窗口移动事件
                            hook = SetWinEventHook(
                                EVENT_OBJECT_LOCATIONCHANGE,
                                EVENT_OBJECT_LOCATIONCHANGE,
                                std::ptr::null_mut(),
                                Some(win_event_proc),
                                w.pid,
                                0,
                                WINEVENT_OUTOFCONTEXT,
                            );
                            hooked_pid = w.pid;
                        }
                        on_change(Some(w.clone()));
                        *slot = Some(w);
                    }
                }
            }

            // ② 快节拍：等钩子事件。有事件立刻醒，没事件 1 秒后醒一次做上面的存活检查。
            //    这就是「移动零延迟、发现一秒内」两个要求各自被满足的地方。
            MsgWaitForMultipleObjectsEx(
                0,
                std::ptr::null(),
                1000,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                DispatchMessageW(&msg);
            }

            if WATCH_DIRTY.with(|d| d.replace(false)) {
                if let Some(c) = current.as_mut() {
                    if let Some(r) = refresh(c) {
                        if c.rect != r {
                            c.rect = r;
                            on_change(Some(c.clone()));
                        }
                    }
                }
            }
        }

        if !hook.is_null() {
            UnhookWinEvent(hook);
        }
    });

    Watcher {
        stop,
        thread: Some(thread),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CODEX_APP;

    #[test]
    fn scanning_processes_does_not_crash_and_is_self_consistent() {
        // 不断言「一定能找到 Codex」—— CI 上没装，那不是失败。
        // 断言的是：找到的每个 pid 都真的能拿到路径，且路径确实含包名标记。
        for pid in pids_of(&CODEX_APP) {
            let p = process_path(pid).expect("既然筛出来了就该拿得到路径");
            assert!(
                p.to_ascii_lowercase().contains(CODEX_APP.path_marker),
                "pid {pid} 的路径不含标记：{p}"
            );
        }
    }

    #[test]
    fn a_bogus_app_matches_nothing() {
        let nobody = HostApp {
            label: "不存在",
            path_marker: "definitely-not-a-real-package-marker",
            exe_names: &["nothing-like-this.exe"],
        };
        assert!(pids_of(&nobody).is_empty());
        assert!(find_host_window(&nobody).is_none());
    }

    #[test]
    fn work_area_is_usable_with_or_without_a_host() {
        ensure_dpi_aware();
        assert!(dpi_awareness_ok(), "没声明 DPI 感知，后面所有坐标都不可信");

        // 没有宿主时取主屏；有宿主时取它所在那块屏。两条路都不许返回空矩形 ——
        // 拿 0×0 去算停靠位置，浮舱会缩成看不见的一条。
        let primary = work_area(None);
        assert!(
            primary.looks_like_a_real_window(),
            "主屏工作区不合理: {primary:?}"
        );

        if let Some(w) = find_host_window(&CODEX_APP) {
            let near = work_area(Some(w.hwnd));
            assert!(
                near.looks_like_a_real_window(),
                "宿主所在屏工作区不合理: {near:?}"
            );
            assert!(
                w.rect.x >= near.x - 1 && w.rect.y >= near.y - 1,
                "宿主不在它自己那块屏里?"
            );
        }

        let at_origin = work_area_at(0, 0);
        assert!(
            at_origin.looks_like_a_real_window(),
            "坐标所在工作区不合理: {at_origin:?}"
        );
    }

    #[test]
    fn window_and_monitor_coordinates_live_in_the_same_space() {
        // 这条守的是一个**静默**错误：进程没声明 DPI 感知时，GetMonitorInfoW 给逻辑像素、
        // DwmGetWindowAttribute 给物理像素。实测 1.25 倍缩放下工作区报 2048（=2560/1.25）、
        // 宿主右边缘报 2507，于是「右边还剩多少」算出 -459px —— 物理上不可能的数，
        // 而它推出的结论恰好还是对的，所以不盯着中间值看根本发现不了。
        ensure_dpi_aware();
        let Some(w) = find_host_window(&CODEX_APP) else {
            return;
        };
        let work = work_area(Some(w.hwnd));

        // MonitorFromWindow 是因为窗口在这块屏上才返回它的，所以两者必须真的相交。
        // 两个坐标系混着时，这个交集会算出负的宽或高。
        let ox = w.rect.right().min(work.right()) - w.rect.x.max(work.x);
        let oy = w.rect.bottom().min(work.bottom()) - w.rect.y.max(work.y);
        assert!(
            ox > 0 && oy > 0,
            "窗口与它所在屏的工作区不相交，坐标系混了: {:?} vs {work:?}",
            w.rect
        );

        // 窗口不该比它所在的屏幕还大出一大截 —— 缩放比最高 1.5 左右，
        // 逻辑/物理混用会造成 25%~50% 的系统性偏差，这个阈值刚好卡在中间。
        assert!(
            w.rect.w <= work.w * 3 / 2 && w.rect.h <= work.h * 3 / 2,
            "窗口({:?})比工作区({work:?})大太多，像是逻辑与物理像素混用",
            w.rect
        );
    }

    #[test]
    fn found_window_is_plausible() {
        // Codex 没开着就跳过；开着的话，找到的必须是个像样的窗口
        let Some(w) = find_host_window(&CODEX_APP) else {
            return;
        };
        assert!(w.rect.looks_like_a_real_window(), "{:?}", w.rect);
        assert!(w.pid > 0);
        assert!(refresh(&w).is_some(), "刚找到的窗口该能刷新出位置");
    }
}
