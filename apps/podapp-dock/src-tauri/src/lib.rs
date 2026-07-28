//! PodApp 浮舱。
//!
//! 一个贴在 Codex 旁边的窄条：把 AI 生成的东西拖进来，选一个确定性动作，
//! 看预览、确认、拿结果。它**不改 Codex、不注入 Codex、不 Fork Codex** ——
//! 只是一个恰好停在它旁边的独立置顶窗口。

mod dock;
mod host;
mod protocol;

use podapp_runtime::{Capabilities, HostProfile, PodInfo};
use podapp_win::Rect;
use serde_json::Value;
use std::collections::HashMap;
use tauri::{Emitter, Manager, PhysicalPosition};
#[cfg(desktop)]
use tauri_plugin_global_shortcut::GlobalShortcutExt;

/// 前端要的一整份状态。做成一次调用而不是四个 getter：
/// 分开取会让界面出现「已吸附但列表还是空的」这种中间态。
#[derive(serde::Serialize)]
pub struct DockStatus {
    pods: Vec<PodInfo>,
    attached: bool,
    host_available: bool,
    host_title: Option<String>,
    expanded: bool,
    placement: &'static str,
    snap_edge: Option<&'static str>,
    /// 装了几个能力提供方，诊断用
    capabilities: Vec<&'static str>,
}

/// 浮舱装了哪些能力。**只在这一处组装** —— 各处各建一份的话，
/// 「界面里能调的动词」和「无头能调的动词」会悄悄不一样，而那正是 parity 要防的。
fn capabilities() -> Capabilities {
    Capabilities::builtin().with(podapp_qr::QrCapability)
}

/// 比较内置小程序版本。官方清单只使用数字点分版本；遇到无法识别的版本时，
/// 宁可不自动覆盖，让用户手动决定。
fn bundled_version_is_newer(candidate: &str, installed: &str) -> bool {
    fn parts(value: &str) -> Option<Vec<u64>> {
        let core = value.trim().trim_start_matches('v').split('-').next()?;
        let parsed: Option<Vec<u64>> = core.split('.').map(|part| part.parse().ok()).collect();
        parsed.filter(|items| !items.is_empty())
    }

    let (Some(mut candidate), Some(mut installed)) = (parts(candidate), parts(installed)) else {
        return false;
    };
    let width = candidate.len().max(installed.len());
    candidate.resize(width, 0);
    installed.resize(width, 0);
    candidate > installed
}

/// 0.1.0 之前的首次安装只是把目录放进 apps，注册表自愈后来源会写成 `adopted`，
/// 也没有 `.install.json`。只凭 ID 覆盖会伤到第三方同名包，所以再核对作者和官方主页。
fn is_legacy_official_pod(id: &str, bundled: &podapp_runtime::manifest::Manifest) -> bool {
    let installed_dir = podapp_runtime::apps_root().join(id);
    let Ok((installed, _)) = podapp_runtime::manifest::load_dir(&installed_dir) else {
        return false;
    };
    bundled.ident.author.as_deref() == Some("PodApp")
        && bundled
            .ident
            .homepage
            .as_deref()
            .is_some_and(|url| url.starts_with("https://podapp.net/pods/"))
        && installed.ident.author == bundled.ident.author
        && installed.ident.homepage == bundled.ident.homepage
}

/// 首次启动装入随应用发布的官方小程序，并升级旧的官方内置版本。
///
/// 只升级注册表来源为 `bundled` 的小程序，手工安装和同 ID 自定义版本不覆盖。
/// 升级后恢复用户的启用与置顶选择。`pods/` 是源码唯一真相源，Tauri 构建时
/// 把它映射到资源目录；开发态则回退到仓库里的原目录。
fn install_missing_bundled_pods(app: &tauri::AppHandle) -> Vec<String> {
    #[cfg(debug_assertions)]
    let _ = app;
    #[cfg(not(debug_assertions))]
    let release_root = app.path().resource_dir().ok().map(|p| p.join("pods"));
    let dev_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../pods");
    // 开发目录里可能留着上一次 Tauri 打包复制的 resources/pods。开发态要始终认
    // 仓库源码，否则改完清单重启仍会装旧版本，视觉上就像改动完全没生效。
    #[cfg(debug_assertions)]
    let root = dev_root;
    #[cfg(not(debug_assertions))]
    let root = release_root.filter(|p| p.is_dir()).unwrap_or(dev_root);
    let installed: HashMap<String, String> = podapp_runtime::registry::list()
        .into_iter()
        .map(|p| (p.id, p.version))
        .collect();
    let entries: HashMap<String, podapp_runtime::registry::RegEntry> =
        podapp_runtime::registry::read()
            .apps
            .into_iter()
            .map(|entry| (entry.id.clone(), entry))
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
        let previous = entries.get(&id);
        let should_install = match installed.get(&id) {
            None => true,
            Some(version) => {
                let official_source = previous.is_some_and(|entry| {
                    entry.source == "bundled"
                        || (entry.source == "adopted" && is_legacy_official_pod(&id, &manifest))
                });
                official_source && bundled_version_is_newer(&manifest.ident.version, version)
            }
        };
        if !should_install {
            continue;
        }
        if let Err(e) = podapp_runtime::install::install_from_path(&dir, "bundled") {
            errors.push(format!("内置小程序 {id} 安装失败: {e}"));
            continue;
        }

        if let Some(previous) = previous {
            let mut registry = podapp_runtime::registry::read();
            if let Some(updated) = registry.apps.iter_mut().find(|entry| entry.id == id) {
                updated.enabled = previous.enabled;
                updated.pinned_home = previous.pinned_home;
            }
            podapp_runtime::registry::write(&registry);
        }
    }
    errors
}

#[tauri::command]
fn dock_status() -> DockStatus {
    let position = dock::placement_summary();
    DockStatus {
        pods: podapp_runtime::registry::list(),
        attached: position.attached,
        host_available: position.host_available,
        host_title: position.host_title,
        expanded: dock::is_expanded(),
        placement: position.placement,
        snap_edge: position.snap_edge,
        capabilities: capabilities().names(),
    }
}

#[tauri::command]
fn dock_expand(app: tauri::AppHandle, on: bool) {
    dock::set_expanded(&app, on);
}

#[tauri::command]
fn dock_finish_drag(app: tauri::AppHandle, x: i32, y: i32) -> dock::DockPlacement {
    dock::finish_drag(&app, x, y)
}

#[tauri::command]
fn dock_begin_drag() {
    dock::begin_drag();
}

#[tauri::command]
fn dock_cancel_drag() {
    dock::cancel_drag();
}

#[tauri::command]
fn dock_restore_free(
    app: tauri::AppHandle,
    x: i32,
    y: i32,
    edge: Option<String>,
) -> dock::DockPlacement {
    dock::restore_free(&app, x, y, edge.as_deref())
}

#[tauri::command]
fn dock_attach(app: tauri::AppHandle) -> dock::DockPlacement {
    dock::attach(&app)
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

fn pod_window_options(id: &str) -> (f64, f64, bool) {
    let fallback = (860.0, 620.0, true);
    let Ok((manifest, _)) =
        podapp_runtime::manifest::load_dir(&podapp_runtime::apps_root().join(id))
    else {
        return fallback;
    };
    let Some(window) = manifest.ui.window.as_ref() else {
        return fallback;
    };
    let width = window
        .get("width")
        .and_then(Value::as_f64)
        .unwrap_or(fallback.0)
        .clamp(360.0, 1600.0);
    let height = window
        .get("height")
        .and_then(Value::as_f64)
        .unwrap_or(fallback.1)
        .clamp(360.0, 1200.0);
    let resizable = window
        .get("resizable")
        .and_then(Value::as_bool)
        .unwrap_or(fallback.2);
    (width, height, resizable)
}

fn clamp_axis(value: i32, start: i32, extent: i32, size: i32) -> i32 {
    if size >= extent {
        start
    } else {
        value.clamp(start, start + extent - size)
    }
}

/// 工具第一次打开时贴着触发它的浮舱；用户拖走后不再强行拉回，等同于“拆成独立面板”。
fn anchored_tool_rect(anchor: Rect, work: Rect, width: i32, height: i32) -> Rect {
    const GAP: i32 = 8;
    let left = anchor.x - width - GAP;
    let right = anchor.right() + GAP;
    let left_fits = left >= work.x;
    let right_fits = right + width <= work.right();
    let prefer_left = anchor.x + anchor.w / 2 >= work.x + work.w / 2;

    let x = match (prefer_left, left_fits, right_fits) {
        (true, true, _) | (false, true, false) => left,
        (false, _, true) | (true, false, true) => right,
        _ => clamp_axis(left, work.x, work.w, width),
    };
    Rect {
        x: clamp_axis(x, work.x, work.w, width),
        y: clamp_axis(anchor.y, work.y, work.h, height),
        w: width,
        h: height,
    }
}

fn hide_other_pod_windows(app: &tauri::AppHandle, keep: &str) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("pod-") && label != keep {
            let _ = window.hide();
        }
    }
}

#[tauri::command]
async fn dock_open_pod(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let info = podapp_runtime::manifest::get(&id)?;
    let label = format!("pod-{}", info.slug);
    hide_other_pod_windows(&app, &label);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.set_focus();
        dock::set_expanded(&app, false);
        return Ok(());
    }
    let (width, height, resizable) = pod_window_options(&id);
    let window = tauri::WebviewWindowBuilder::new(&app, &label, pod_webview_url(&id)?)
        .title(&info.name)
        .inner_size(width, height)
        .resizable(resizable)
        .visible(false)
        .build()
        .map_err(|e| format!("打开失败: {e}"))?;

    // 先收成最终的小船尺寸，再取锚点。反过来会按展开面板定位，收起后留下整段面板宽度的空隙。
    dock::set_expanded(&app, false);
    if let Ok(size) = window.outer_size() {
        let target = anchored_tool_rect(
            dock::target_rect(),
            dock::current_work_area(),
            size.width as i32,
            size.height as i32,
        );
        let _ = window.set_position(PhysicalPosition::new(target.x, target.y));
    }
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

#[tauri::command]
fn dock_developer_prompt() -> &'static str {
    include_str!("../../../../docs/POD-DEVELOPMENT.md")
}

#[tauri::command]
fn dock_skin_prompt() -> &'static str {
    include_str!("../../../../docs/SKIN-DEVELOPMENT.md")
}

#[cfg(test)]
mod pod_window_tests {
    #[test]
    fn bundled_versions_only_move_forward() {
        assert!(super::bundled_version_is_newer("0.2.0", "0.1.9"));
        assert!(super::bundled_version_is_newer("1.0", "0.9.9"));
        assert!(!super::bundled_version_is_newer("0.2.0", "0.2.0"));
        assert!(!super::bundled_version_is_newer("0.1.9", "0.2.0"));
        assert!(!super::bundled_version_is_newer("next", "0.2.0"));
    }

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

    #[test]
    fn tool_window_anchors_beside_a_right_hand_dock() {
        let work = podapp_win::Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1040,
        };
        let dock = podapp_win::Rect {
            x: 1850,
            y: 120,
            w: 70,
            h: 64,
        };
        assert_eq!(
            super::anchored_tool_rect(dock, work, 420, 560),
            podapp_win::Rect {
                x: 1422,
                y: 120,
                w: 420,
                h: 560
            }
        );
    }

    #[test]
    fn tool_window_stays_inside_the_work_area() {
        let work = podapp_win::Rect {
            x: -1280,
            y: 0,
            w: 1280,
            h: 984,
        };
        let dock = podapp_win::Rect {
            x: -1280,
            y: 900,
            w: 70,
            h: 64,
        };
        let target = super::anchored_tool_rect(dock, work, 1000, 720);
        assert!(target.x >= work.x && target.right() <= work.right());
        assert!(target.y >= work.y && target.bottom() <= work.bottom());
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
/// 拆出纯函数是为了能测 —— 直接读 `std::env::args()` 的版本没法在测试里喂参数，
/// 而「哪些参数会被当成包装上」正是这里最该被钉住的一条。
fn pods_from_args<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .skip(1)
        .filter(|a| a.to_ascii_lowercase().ends_with(".pod") && std::path::Path::new(a).is_file())
        .collect()
}

fn pods_from_argv() -> Vec<String> {
    pods_from_args(std::env::args())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 运行时装 PodApp 档案：`~/.podapp`、`PODAPP_*` 环境变量、`window.pod` 桥。
    // 失败只可能是被装过了（测试里会），不是错误。
    let _ = podapp_runtime::init(HostProfile::podapp(env!("CARGO_PKG_VERSION")));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // 自更新。**端点是多个、按顺序回退的**：国内域名在前、GitHub 兜底 ——
        // 只指 GitHub 的话国内用户基本更新不到（U-King 的 installer.rs 里有同样的
        // 教训注释：cloud.u-claw.org 在部分网络会被 SNI reset）。
        // 开发机有代理，感觉不到这件事，所以这条不能靠「我这能行」来验。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .register_uri_scheme_protocol("podapp", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            dock_status,
            dock_expand,
            dock_begin_drag,
            dock_cancel_drag,
            dock_finish_drag,
            dock_restore_free,
            dock_attach,
            dock_install,
            dock_uninstall,
            dock_run,
            dock_open_pod,
            dock_developer_prompt,
            dock_skin_prompt,
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
                win.on_window_event(move |e| match e {
                    tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                        let files: Vec<String> =
                            paths.iter().map(|p| p.display().to_string()).collect();
                        let _ = h.emit("dock://dropped", files);
                    }
                    tauri::WindowEvent::Moved(position) => {
                        dock::note_window_moved(h.clone(), position.x, position.y);
                    }
                    _ => {}
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

            // 双击 .pod 拉起我们时，把它装上并展开给用户看结果。
            //
            // **装这件事必须在这里、用运行时直接做，不能发事件交给界面去装。**
            // `setup` 跑在 webview 加载之前，`emit` 是发完即忘 —— 那一刻界面还没注册监听，
            // 事件当场丢掉，于是浮舱正常弹出、包却一个都没装上，看起来像「什么也没发生」。
            // 这正是「业务动作只实现一次、装在面之下」要挡的分叉：装包是业务动作，
            // 走的必须是 `dock_install` 同一个函数，而不是界面事件总线。
            // 装完不必额外通知界面：前端起来后自己会 refresh 一次，列表就是新的。
            let pending = pods_from_argv();
            if !pending.is_empty() {
                for path in &pending {
                    match podapp_runtime::install::install_from_path(
                        std::path::Path::new(path),
                        "argv",
                    ) {
                        Ok(info) => eprintln!("[dock] 已装 {} v{}", info.id, info.version),
                        Err(e) => eprintln!("[dock] 装 {path} 失败：{e}"),
                    }
                }
                dock::set_expanded(&handle, true);
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
                    &podapp_runtime::Invocation::new(
                        id,
                        args.get("input").cloned().unwrap_or(Value::Null),
                    ),
                    host,
                    &caps,
                    None,
                );
            }
        }
    }
    caps.dispatch(
        &CapCtx {
            pod_id,
            host,
            execution_id: "",
        },
        verb,
        args,
    )
}

#[cfg(test)]
mod argv_tests {
    use super::pods_from_args;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("podapp-argv-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &std::path::Path, name: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, b"not a real package, only needs to exist").unwrap();
        p.display().to_string()
    }

    /// 双击 `.pod` 是「谁能让 PodApp 装东西」的入口，所以这道过滤是**安全闸**，不是便利：
    /// 只有真实存在的 `.pod` 文件能过，别的参数一律不当包。
    #[test]
    fn only_existing_pod_files_are_installed() {
        let dir = scratch("only-existing");
        let real = touch(&dir, "real.pod");
        let not_a_pod = touch(&dir, "readme.txt");
        let looks_like_pod = touch(&dir, "trap.pod.txt");
        let missing = dir.join("ghost.pod").display().to_string();

        let got = pods_from_args(vec![
            "podapp-dock.exe".into(),
            real.clone(),
            not_a_pod,
            looks_like_pod,
            missing,
            "--flag".into(),
            String::new(),
        ]);

        assert_eq!(got, vec![real]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// argv[0] 是自己的可执行文件路径。要是有人把 exe 改名成 `x.pod`，
    /// 跳过第一个参数这件事就从「习惯」变成「必须」—— 否则浮舱会去装自己。
    #[test]
    fn own_executable_path_is_never_installed() {
        let dir = scratch("argv0");
        let self_path = touch(&dir, "podapp-dock.pod");

        assert!(pods_from_args(vec![self_path]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Windows 的文件名大小写不敏感，`.POD` 和 `.pod` 是同一个东西。
    #[test]
    fn extension_match_ignores_case() {
        let dir = scratch("case");
        let shouty = touch(&dir, "LOUD.POD");

        let got = pods_from_args(vec!["podapp-dock.exe".into(), shouty.clone()]);
        assert_eq!(got, vec![shouty]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 不带参数直接启动是最常见的用法，这时一个包都不该被装。
    #[test]
    fn bare_launch_installs_nothing() {
        assert!(pods_from_args(vec!["podapp-dock.exe".to_string()]).is_empty());
    }
}
