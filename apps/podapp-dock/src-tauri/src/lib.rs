//! PodApp 浮舱。
//!
//! 一个贴在 Codex 旁边的窄条：把 AI 生成的东西拖进来，选一个确定性动作，
//! 看预览、确认、拿结果。它**不改 Codex、不注入 Codex、不 Fork Codex** ——
//! 只是一个恰好停在它旁边的独立置顶窗口。

mod dock;
mod host;
mod protocol;

use podapp_runtime::{HostProfile, PodInfo};
use serde_json::Value;
use tauri::{Emitter, Manager};

/// 前端要的一整份状态。做成一次调用而不是四个 getter：
/// 分开取会让界面出现「已吸附但列表还是空的」这种中间态。
#[derive(serde::Serialize)]
pub struct DockStatus {
    pods: Vec<PodInfo>,
    attached: bool,
    host_title: Option<String>,
    expanded: bool,
    /// 装了几个能力提供方，诊断用
    capabilities: Vec<&'static str>,
}

#[tauri::command]
fn dock_status() -> DockStatus {
    let host = dock::host_summary();
    DockStatus {
        pods: podapp_runtime::registry::list(),
        attached: host.is_some(),
        host_title: host.map(|(t, _)| t),
        expanded: dock::is_expanded(),
        capabilities: podapp_runtime::Capabilities::builtin().names(),
    }
}

#[tauri::command]
fn dock_expand(app: tauri::AppHandle, on: bool) {
    dock::set_expanded(&app, on);
}

/// 安装一个 `.pod`（或已解包的目录）。
#[tauri::command]
fn dock_install(path: String) -> Result<PodInfo, String> {
    podapp_runtime::install::install_from_path(std::path::Path::new(&path), "dock-drop")
}

#[tauri::command]
fn dock_uninstall(id: String, purge_data: bool) -> Result<(), String> {
    podapp_runtime::install::uninstall(&id, purge_data)
}

/// 无头跑一个动作。GUI 里点按钮和 AI 无头调用**走的是同一个函数**。
#[tauri::command]
fn dock_run(action_id: String, input: Value) -> Result<Value, String> {
    podapp_runtime::headless::run_action_with(&action_id, input, &host::DockHost)
}

/// 打开一个程序舱的界面。
///
/// **开独立窗口，不嵌 iframe。** WebView2 会拒绝跨 scheme 的 iframe
///（U-King 0.9.72 那次 c29590d 就栽在这上面：容器永远卡在「正在打开」）。
/// 这不是取舍，是平台限制，第一版直接按这个形态设计。
#[tauri::command]
fn dock_open_pod(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let info = podapp_runtime::manifest::get(&id)?;
    let label = format!("pod-{}", info.slug);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = format!("podapp://localhost/app/{id}/");
    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::External(url.parse().map_err(|e| format!("地址不合法: {e}"))?),
    )
    .title(&info.name)
    .inner_size(1000.0, 720.0)
    .build()
    .map_err(|e| format!("打开失败: {e}"))?;
    Ok(())
}

#[tauri::command]
fn dock_artifacts() -> Vec<podapp_runtime::artifacts::Artifact> {
    podapp_runtime::artifacts::list()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 运行时装 PodApp 档案：`~/.podapp`、`PODAPP_*` 环境变量、`window.pod` 桥。
    // 失败只可能是被装过了（测试里会），不是错误。
    let _ = podapp_runtime::init(HostProfile::podapp(env!("CARGO_PKG_VERSION")));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_uri_scheme_protocol("podapp", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            dock_status,
            dock_expand,
            dock_install,
            dock_uninstall,
            dock_run,
            dock_open_pod,
            dock_artifacts,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // 跟随线程要一直活着 —— watcher 一 drop 就停了，浮舱会静止在最后一个位置，
            // 而那看起来像「卡住了」而不是「跟随坏了」。挂到 app state 上随进程走。
            let watcher = dock::start_following(handle.clone());
            app.manage(watcher);

            // 拖文件进来就装。这是「拖入即处理」的第一步，也是分发 .pod 的主路径。
            let h = handle.clone();
            if let Some(win) = app.get_webview_window(dock::DOCK_LABEL) {
                win.on_window_event(move |e| {
                    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop {
                        paths, ..
                    }) = e
                    {
                        let files: Vec<String> =
                            paths.iter().map(|p| p.display().to_string()).collect();
                        let _ = h.emit("dock://dropped", files);
                    }
                });
            }

            // 必须在第一次 reposition 之前 —— 否则收起态会被系统下限撑到 170px
            dock::allow_narrow(&handle);
            dock::reposition(&handle);
            if let Some(win) = app.get_webview_window(dock::DOCK_LABEL) {
                let _ = win.show();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("浮舱起不来");
}
