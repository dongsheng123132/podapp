//! 证明「跟随」真的跟得上 —— 用一个自己造的窗口，不碰用户的 Codex。
//!
//! 为什么非要这条测试：`find_host_window` 好验（跑一次看结果对不对），但**跟随**的失败模式
//! 是安静的 —— 钩子没装上、事件被过滤掉、目标 hwnd 比对写反，表现都一样：
//! 「浮舱就是不动」。而开发机上你多半会以为是自己没拖对窗口。
//!
//! 所以这里造一个真窗口、真移动它、断言回调真的收到了新位置。
//! 用自己进程里的窗口而不是找个现成的应用：不依赖机器上装了什么，也不动用户的任何状态。

#![cfg(windows)]

use podapp_win::{refresh, watch, HostApp, HostWindow, Rect};
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassW,
    SetWindowPos, ShowWindow, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNOACTIVATE,
    WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 造一个可见的顶层窗口，返回 hwnd。尺寸要过 `looks_like_a_real_window` 那道筛子。
unsafe fn make_window() -> HWND {
    let class = wide("PodAppFollowTestWindow");
    let hinst = GetModuleHandleW(std::ptr::null());
    let mut wc: WNDCLASSW = std::mem::zeroed();
    wc.lpfnWndProc = Some(DefWindowProcW);
    wc.hInstance = hinst;
    wc.lpszClassName = class.as_ptr();
    RegisterClassW(&wc); // 重复注册返回 0，无所谓

    let title = wide("podapp follow test");
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPEDWINDOW,
        100,
        100,
        600,
        400,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    hwnd
}

/// 窗口在本线程上，得有人替它抽消息，否则它不响应也不会真正完成布局。
unsafe fn pump() {
    let mut msg: MSG = std::mem::zeroed();
    while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
        DispatchMessageW(&msg);
    }
}

#[test]
fn moving_the_host_window_reaches_the_callback() {
    // 目标就是当前测试进程自己
    let exe = std::env::current_exe().unwrap();
    let name = exe.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
    let leaked_name: &'static str = Box::leak(name.into_boxed_str());
    let app = HostApp {
        label: "自造测试窗口",
        // 路径标记用可执行文件名本身：一定匹配，且不会误抓别的进程
        path_marker: leaked_name,
        exe_names: Box::leak(vec![leaked_name].into_boxed_slice()),
    };

    let hwnd = unsafe { make_window() };
    unsafe { pump() };

    let (tx, rx) = mpsc::channel::<Option<Rect>>();
    let _watcher = watch(app, Box::new(move |w| { let _ = tx.send(w.map(|w| w.rect)); }));

    // 等一个满足条件的上报。**边等边抽消息** —— 窗口在本线程上，一路阻塞地干等
    // 就是在制造跨线程死锁（跟随线程若要问这个窗口点什么，只能等我们抽消息）。
    let wait_for = |what: &str, pred: &dyn Fn(Rect) -> bool| -> Rect {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            assert!(std::time::Instant::now() < deadline, "5 秒内没等到{what}");
            unsafe { pump() };
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Some(r)) if pred(r) => break r,
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(e) => panic!("跟随线程没了: {e}"),
            }
        }
    };

    let first = wait_for("发现自己的窗口", &|r| r.looks_like_a_real_window());

    // 移动三次，每次都要收到新位置。三次而不是一次：一次可能是发现阶段的重复上报蒙对的。
    let mut last = first;
    for i in 1..=3 {
        let (nx, ny) = (140 + i * 60, 120 + i * 40);
        unsafe {
            SetWindowPos(hwnd, std::ptr::null_mut(), nx, ny, 600, 400, SWP_NOZORDER | SWP_NOACTIVATE);
            pump();
        }
        let moved = wait_for(&format!("第 {i} 次移动后的位置变化（上次 {last:?}）"), &|r| r != last);

        // 刻意**不**断言 `moved == (nx, ny)`。实测这台机器上设 (200,160) 报回 (259,200)：
        // 1.25 倍 DPI 虚拟化，外加 x 方向 9px 的 DWM 不可见边框（y 方向没有）。
        // 那些数字是 Windows 的坐标换算，不是跟随功能的正确性 —— 把它们写进断言，
        // 测试就会在换台屏幕、换个缩放比例时无故变红，而功能其实好好的。
        //
        // 真正要证的是两件事：上报的位置**就是这个窗口此刻的真实位置**，
        // 而且它确实跟着我们的移动走了。
        let truth = refresh(&HostWindow { hwnd: hwnd as isize, pid: 0, title: String::new(), rect: moved })
            .expect("窗口还在，该刷得出位置");
        assert_eq!(moved, truth, "第 {i} 次上报的位置和窗口真实位置对不上");
        assert!(moved.x > last.x && moved.y > last.y, "第 {i} 次：往右下移动了，上报却没跟着走");
        last = moved;
    }

    unsafe {
        DestroyWindow(hwnd);
        pump();
    }
}
