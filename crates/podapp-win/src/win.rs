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
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsWindowVisible, SendMessageTimeoutW, GW_OWNER, SMTO_ABORTIFHUNG, WM_GETTEXT,
};

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    std::ffi::OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
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
        Some(Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top })
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
    let Some(rect) = window_rect(hwnd) else { return 1 };
    if !rect.looks_like_a_real_window() {
        return 1;
    }
    scan.found.push(HostWindow { hwnd: hwnd as isize, pid, title: window_title(hwnd), rect });
    1
}

/// 找宿主应用的主窗口。找不到返回 `None` —— 宿主没开着是**常态**，不是错误。
///
/// 多个候选时挑面积最大的那个：Chromium 系应用除了主窗口还会有若干辅助顶层窗口，
/// 主窗口总是最大的那个。
pub fn find_host_window(app: &HostApp) -> Option<HostWindow> {
    let pids = pids_of(app);
    if pids.is_empty() {
        return None;
    }
    let mut scan = Scan { pids, found: vec![] };
    unsafe {
        EnumWindows(Some(enum_proc), &mut scan as *mut Scan as LPARAM);
    }
    scan.found.into_iter().max_by_key(|w| (w.rect.w as i64) * (w.rect.h as i64))
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
        let mut hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK =
            std::ptr::null_mut();
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

    Watcher { stop, thread: Some(thread) }
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
    fn found_window_is_plausible() {
        // Codex 没开着就跳过；开着的话，找到的必须是个像样的窗口
        let Some(w) = find_host_window(&CODEX_APP) else { return };
        assert!(w.rect.looks_like_a_real_window(), "{:?}", w.rect);
        assert!(w.pid > 0);
        assert!(refresh(&w).is_some(), "刚找到的窗口该能刷新出位置");
    }
}
