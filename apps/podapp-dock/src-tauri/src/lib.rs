//! PodApp 浮舱。
//!
//! 一个贴在 Codex 旁边的窄条：把 AI 生成的东西拖进来，选一个确定性动作，
//! 看预览、确认、拿结果。它**不改 Codex、不注入 Codex、不 Fork Codex** ——
//! 只是一个恰好停在它旁边的独立置顶窗口。

mod dock;
mod host;
mod protocol;

use podapp_runtime::{Capabilities, HostProfile, PodInfo};
use serde_json::Value;
use std::collections::HashSet;
use tauri::{Emitter, Manager};
#[cfg(desktop)]
use tauri_plugin_global_shortcut::GlobalShortcutExt;

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

/// 浮舱装了哪些能力。**只在这一处组装** —— 各处各建一份的话，
/// 「界面里能调的动词」和「无头能调的动词」会悄悄不一样，而那正是 parity 要防的。
fn capabilities() -> Capabilities {
    Capabilities::builtin().with(podapp_qr::QrCapability)
}

/// 首次启动装入随应用发布的官方小程序。
///
/// 只补缺失项，不覆盖用户已安装的同 ID 小程序。`pods/` 是源码唯一真相源，
/// Tauri 构建时把它映射到资源目录；开发态则回退到仓库里的原目录。
fn install_missing_bundled_pods(app: &tauri::AppHandle) -> Vec<String> {
    let release_root = app.path().resource_dir().ok().map(|p| p.join("pods"));
    let dev_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../pods");
    let root = release_root.filter(|p| p.is_dir()).unwrap_or(dev_root);
    let mut installed: HashSet<String> = podapp_runtime::registry::list()
        .into_iter()
        .map(|p| p.id)
        .collect();
    let mut errors = Vec::new();

    let mut dirs: Vec<_> = match std::fs::read_dir(&root) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => {
            errors.push(format!("读不到内置小程序目录 {}: {e}", root.display()));
            return errors;
        }
    };
    dirs.sort();

    for dir in dirs {
        let manifest = match podapp_runtime::manifest::load_dir(&dir) {
            Ok((manifest, _)) => manifest,
            Err(e) => {
                errors.push(format!("内置小程序 {} 无效: {e}", dir.display()));
                continue;
            }
        };
        let id = manifest.ident.id.clone();
        if installed.contains(&id) {
            continue;
        }
        if let Err(e) = podapp_runtime::install::install_from_path(&dir, "bundled") {
            errors.push(format!("内置小程序 {id} 安装失败: {e}"));
        } else {
            installed.insert(id);
        }
    }
    errors
}

#[tauri::command]
fn dock_status() -> DockStatus {
    let host = dock::host_summary();
    DockStatus {
        pods: podapp_runtime::registry::list(),
        attached: host.is_some(),
        host_title: host.map(|(t, _)| t),
        expanded: dock::is_expanded(),
        capabilities: capabilities().names(),
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
    podapp_runtime::headless::invoke(
        &podapp_runtime::Invocation::new(&action_id, input),
        &host::DockHost,
        &capabilities(),
        None,
    )
}

/// 打开一个程序舱的界面。
///
/// **开独立窗口，不嵌 iframe。** WebView2 会拒绝跨 scheme 的 iframe
///（U-King 0.9.72 那次 c29590d 就栽在这上面：容器永远卡在「正在打开」）。
/// 这不是取舍，是平台限制，第一版直接按这个形态设计。
fn pod_webview_url(id: &str) -> Result<tauri::WebviewUrl, String> {
    let url = format!("podapp://localhost/app/{id}/");
    Ok(tauri::WebviewUrl::CustomProtocol(
        url.parse().map_err(|e| format!("地址不合法: {e}"))?,
    ))
}

#[tauri::command]
async fn dock_open_pod(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let info = podapp_runtime::manifest::get(&id)?;
    let label = format!("pod-{}", info.slug);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        pod_webview_url(&id)?,
    )
    .title(&info.name)
    .inner_size(1000.0, 720.0)
    .build()
    .map_err(|e| format!("打开失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod pod_window_tests {
    #[test]
    fn pod_pages_are_custom_protocol_urls() {
        match super::pod_webview_url("org.podapp.image.nine-grid").unwrap() {
            tauri::WebviewUrl::CustomProtocol(url) => {
                assert_eq!(
                    url.as_str(),
                    "podapp://localhost/app/org.podapp.image.nine-grid/"
                );
            }
            other => panic!("小程序页面不能作为外部 URL 打开: {other:?}"),
        }
    }
}

#[tauri::command]
fn dock_artifacts() -> Vec<podapp_runtime::artifacts::Artifact> {
    podapp_runtime::artifacts::list()
}

/// 命令行里带的 `.pod` 路径。
///
/// 双击 `.pod` 时 Windows 就是把路径当参数拉起我们。**只认 `.pod` 结尾的存在的文件** ——
/// 把任意参数都当包去装，等于给「谁能让 PodApp 装东西」开了一个没上锁的门。
fn pods_from_argv() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| a.to_ascii_lowercase().ends_with(".pod") && std::path::Path::new(a).is_file())
        .collect()
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

            for error in install_missing_bundled_pods(&handle) {
                eprintln!("[dock] {error}");
            }

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

            // 全局热键：不管焦点在哪都能把浮舱叫出来。这是「浮在界面之上」的另一半 ——
            // 吸附解决「它在哪」，热键解决「怎么够得着」。
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
                let toggle = Shortcut::new(Some(Modifiers::ALT), Code::Space);
                let h = handle.clone();
                let plugin = tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |_app, sc, event| {
                        // 只认按下，不认抬起 —— 否则一次按键会切换两回，看起来像没反应
                        if sc == &toggle && event.state() == ShortcutState::Pressed {
                            dock::set_expanded(&h, !dock::is_expanded());
                        }
                    })
                    .build();
                // 热键被别的软件占了是常见情况，**不该让浮舱起不来** —— 记一笔继续走
                if let Err(e) = handle.plugin(plugin) {
                    eprintln!("[dock] 全局热键插件没装上：{e}");
                } else if let Err(e) = handle.global_shortcut().register(toggle) {
                    eprintln!("[dock] Alt+Space 注册失败（多半被别的软件占了）：{e}");
                }
            }

            dock::reposition(&handle);
            if let Some(win) = app.get_webview_window(dock::DOCK_LABEL) {
                let _ = win.show();
            }

            // 双击 .pod 拉起我们时，把它装上并展开给用户看结果
            let pending = pods_from_argv();
            if !pending.is_empty() {
                dock::set_expanded(&handle, true);
                let _ = handle.emit("dock://dropped", pending);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("浮舱起不来");
}

/// 界面侧的一次能力请求，用浮舱装配的能力集。
///
/// `podapp_runtime::headless::rpc` 用的是内置能力集，看不到浮舱额外注册的
/// `qr.*` —— 界面能调而无头调不到（或反过来）正是 parity 要消灭的那种分叉，
/// 所以两条路都从 [`capabilities`] 拿同一份。
pub(crate) fn rpc_with_dock_capabilities(
    pod_id: &str,
    verb: &str,
    args: &Value,
    host: &dyn podapp_runtime::HostBridge,
) -> Result<Value, String> {
    use podapp_runtime::capability::CapCtx;
    let caps = capabilities();
    // 程序舱调自己的动作仍走动作总线，其余才是能力调用
    if verb == "action" {
        if let Some(id) = args.get("id").and_then(|v| v.as_str()) {
            if podapp_runtime::manifest::owner_of(id).as_deref() == Some(pod_id) {
                return podapp_runtime::headless::invoke(
                    &podapp_runtime::Invocation::new(id, args.get("input").cloned().unwrap_or(Value::Null)),
                    host,
                    &caps,
                    None,
                );
            }
        }
    }
    caps.dispatch(&CapCtx { pod_id, host, execution_id: "" }, verb, args)
}
