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
    EnumWindows, GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
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

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        wide_to_string(&buf[..got.max(0) as usize])
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
