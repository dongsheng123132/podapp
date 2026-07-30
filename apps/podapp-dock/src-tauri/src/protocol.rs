//! `podapp://` 协议处理器 —— 把运行时接到 WebView 上。
//!
//! 五条路径，全都收敛到别处，这里不做第二份实现：
//!
//! | 路径 | 干什么 |
//! |---|---|
//! | `/__podapp/bridge.js` | 桥脚本 |
//! | `/app/<pod-id>/<rel>` | 程序舱静态资源（入口页会被注入桥） |
//! | `/rpc/<pod-id>/<verb>` | 能力调用（POST） |
//! | `/artifact/<id>` | 产物字节 |
//! | `/pet/<pet-id>/sprite` | Codex 宠物图集字节 |
//! | `/win/<pod-id>/<verb>` | 自己那扇窗的拖动与关闭（POST） |
//!
//! **CSP 由 [`podapp_runtime::perms::csp_for`] 逐个程序舱下发**，不在这里写死。
//! 写死就意味着「这个程序舱申请了哪些网络源」这件事有两份定义。

use podapp_runtime::{bridge, manifest, perms, serve};
use tauri::http::{Request, Response};
// `get_webview_window` 挂在 Manager trait 上，不 use 进来的报错是
// 「AppHandle 没有这个方法」—— 看起来像 API 变了，其实只是 trait 没进作用域
use tauri::Manager;

/// 从 `/app/<pod-id>/<rel...>` 里拆出 pod-id 和相对路径。
fn split_pod_path<'a>(path: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = path.strip_prefix(prefix)?;
    match rest.split_once('/') {
        Some((id, rel)) => Some((id, rel)),
        None => Some((rest, "")),
    }
}

/// 这次请求是不是那个程序舱**自己**发的。
///
/// 路径里的 pod-id 是页面自己填的字符串，谁都能改；`webview_label` 由宿主建窗时
/// 定下（`dock_open_pod` 里的 `pod-{slug}`），页面碰不到。所以身份只认标签。
///
/// **这不只是窗口的事。** `/rpc/` 原来直接信路径里的 id，而那个 id 一路传到
/// `perms::permits(pod_id, cap)` 和 `data_dir(pod_id)` —— 也就是说 A 舱的页面
/// `fetch("/rpc/<B的id>/storage.get")` 查的是 B 的清单、读的是 B 的目录，
/// A 自己什么权限都不用申请。`storage.set` 还能覆盖 B 的数据。
///
/// 抽成纯函数是因为它是这条路上**唯一**会悄悄失效的地方：拼法在别处改了而这里
/// 没跟上，判断会一律 false（还好）或一律 true（灾难），两种都不报错。
fn is_own_surface(pod_slug: &str, webview_label: &str) -> bool {
    !pod_slug.is_empty() && format!("pod-{pod_slug}") == webview_label
}

/// 核对请求方身份，不对就给出能照着查的拒绝。
fn deny_unless_own(pod_id: &str, webview_label: &str) -> Option<Response<Vec<u8>>> {
    let ok = manifest::get(pod_id)
        .map(|info| is_own_surface(&info.slug, webview_label))
        .unwrap_or(false);
    (!ok).then(|| {
        json_result(Err(format!(
            "identity_denied: {webview_label} 不是 {pod_id}，不能替它调用"
        )))
    })
}

fn text(status: u16, body: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(body.as_bytes().to_vec())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn json_result(r: Result<serde_json::Value, String>) -> Response<Vec<u8>> {
    // 统一信封：`{ok, data}` / `{ok, error}`。桥那边只认这一种形状。
    let (status, body) = match r {
        Ok(d) => (200u16, serde_json::json!({ "ok": true, "data": d })),
        Err(e) => (200u16, serde_json::json!({ "ok": false, "error": e })),
    };
    // 注意状态码固定 200：错误走信封而不是 HTTP 状态。fetch 那边只有一条成功路径，
    // 分成两条（HTTP 错误 + 信封错误）会让每个调用点都要写两遍错误处理。
    Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .body(body.to_string().into_bytes())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

pub fn handle<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    req: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let _ = ctx;
    let path = req.uri().path().to_string();

    if path == bridge::script_path() {
        return Response::builder()
            .status(200)
            .header("content-type", "text/javascript; charset=utf-8")
            .body(bridge::script().into_bytes())
            .unwrap_or_else(|_| Response::new(Vec::new()));
    }

    if let Some((pod_id, verb)) = split_pod_path(&path, "/rpc/") {
        if verb.is_empty() {
            return text(400, "rpc 缺少动词");
        }
        // 身份先核，再谈权限。**顺序不能反** —— 先按路径里的 id 查权限，
        // 查的就已经是别人的清单了。
        if let Some(denied) = deny_unless_own(pod_id, ctx.webview_label()) {
            return denied;
        }
        let args: serde_json::Value =
            serde_json::from_slice(req.body()).unwrap_or(serde_json::Value::Null);
        let host = crate::host::DockHost;
        // 走浮舱那一份能力集 —— 界面这条路和无头那条路必须看到同样的动词
        return json_result(crate::rpc_with_dock_capabilities(
            pod_id, verb, &args, &host,
        ));
    }

    // 窗口自带动作。**只准动自己那扇窗。**
    //
    // 路径里的 pod-id 是页面自己填的，骗得过；`webview_label` 是宿主建窗时定的，
    // 骗不过。所以归属核对只认标签 —— 拿路径里的 id 去信，等于让任何程序舱
    // 都能关掉别人的窗，而这种事发生时用户只会觉得「泊舟自己乱关窗」。
    if let Some((pod_id, verb)) = split_pod_path(&path, "/win/") {
        if let Some(denied) = deny_unless_own(pod_id, ctx.webview_label()) {
            return denied;
        }
        let Some(window) = ctx.app_handle().get_webview_window(ctx.webview_label()) else {
            return json_result(Err("window_missing: 找不到这扇窗".into()));
        };
        return json_result(match verb {
            // 交给系统拖：自己算坐标要处理 DPI 缩放，而混用逻辑/物理像素
            // 是这个项目栽过的坑（AGENTS.md 5）
            "drag" => window
                .start_dragging()
                .map(|_| serde_json::Value::Null)
                .map_err(|e| format!("拖不动: {e}")),
            "close" => window
                .close()
                .map(|_| serde_json::Value::Null)
                .map_err(|e| format!("关不掉: {e}")),
            other => Err(format!("window_unknown_verb: {other}")),
        });
    }

    if let Some(id) = path.strip_prefix("/artifact/") {
        return match podapp_runtime::artifacts::read_bytes(id) {
            Some(b) => Response::builder()
                .status(200)
                .header("content-type", "application/octet-stream")
                .body(b)
                .unwrap_or_else(|_| Response::new(Vec::new())),
            None => text(404, "没有这个产物"),
        };
    }

    // 宠物图集。**只认 `/pet/<id>/sprite` 这一条**，不做 `/pet/<id>/<任意文件>` ——
    // 后者等于把宠物目录当静态站点端出去，而那个目录里将来会有什么谁也说不准。
    if let Some((pet_id, rest)) = split_pod_path(&path, "/pet/") {
        if rest != "sprite" {
            return text(404, "宠物只提供 /sprite");
        }
        return match crate::pet_sprite_bytes(pet_id) {
            Ok((bytes, mime)) => Response::builder()
                .status(200)
                .header("content-type", mime)
                // 图集在宠物的生命周期里不变，但用户可能刚用 hatch-pet 重做了一版。
                // 让 WebView 每次带条件请求过来，比缓存住一张过时的图强 ——
                // 「我明明改了它还是老样子」是最难自己排查的一类现象。
                .header("cache-control", "no-cache")
                .body(bytes)
                .unwrap_or_else(|_| Response::new(Vec::new())),
            Err(e) => text(404, &e),
        };
    }

    if let Some((pod_id, rel)) = split_pod_path(&path, "/app/") {
        let served = serve::serve(pod_id, rel);
        let is_entry = served.status == 200 && served.mime.starts_with("text/html");
        // 桥只注入 HTML 文档。注入到 js/png 上会把文件内容弄坏，
        // 而「图片打不开」根本不会让人联想到桥。
        let body = if is_entry {
            bridge::inject(&served.body, pod_id)
        } else {
            served.body
        };

        let csp = manifest::permissions(pod_id)
            .map(|p| perms::csp_for(&p))
            .unwrap_or_default();
        let mut b = Response::builder()
            .status(served.status)
            .header("content-type", served.mime);
        if is_entry && !csp.is_empty() {
            b = b.header("content-security-policy", csp);
        }
        if served.no_store {
            b = b.header("cache-control", "no-store");
        }
        return b.body(body).unwrap_or_else(|_| Response::new(Vec::new()));
    }

    text(404, "podapp:// 不认识这个路径")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_paths_split_correctly() {
        assert_eq!(
            split_pod_path("/app/org.x.y/web/a.js", "/app/"),
            Some(("org.x.y", "web/a.js"))
        );
        // 没有尾部路径 = 入口页
        assert_eq!(
            split_pod_path("/app/org.x.y", "/app/"),
            Some(("org.x.y", ""))
        );
        assert_eq!(
            split_pod_path("/app/org.x.y/", "/app/"),
            Some(("org.x.y", ""))
        );
        assert_eq!(
            split_pod_path("/rpc/org.x.y/image.decode", "/rpc/"),
            Some(("org.x.y", "image.decode"))
        );
        assert_eq!(split_pod_path("/other", "/app/"), None);
    }

    /// 宠物那条路只开 `/sprite` 一个口子。
    ///
    /// 拆出来的第二段必须原样是 `sprite` —— 一旦这里放行了别的尾巴，
    /// 宠物目录就等于被当成静态站点端出去了，而那个目录里将来会有什么谁也说不准。
    #[test]
    fn a_pet_only_exposes_its_sprite() {
        assert_eq!(split_pod_path("/pet/ember/sprite", "/pet/"), Some(("ember", "sprite")));
        // 这几种都会被 handle 挡在 404：拆得出来，但第二段不是 sprite
        for path in ["/pet/ember/pet.json", "/pet/ember", "/pet/ember/a/b"] {
            let (_, rest) = split_pod_path(path, "/pet/").expect("应当拆得出来");
            assert_ne!(rest, "sprite", "{path} 不该被当成图集请求");
        }
    }

    /// 一个程序舱只能替**自己**调用 —— 既包括动窗口，也包括 `/rpc/` 上的能力。
    ///
    /// 实机验过窗口那半（悬浮件去关二维码 Pod 的窗 → 被拒，关自己 → 关掉了），
    /// 而且日志确认过 pod 页面的 webview 标签就是 `pod-<slug>`。
    /// 但实机只证明当天对，这条守住的是拼法本身。
    #[test]
    fn a_pod_can_only_act_as_itself() {
        assert!(is_own_surface("floattest", "pod-floattest"));
        // 冒别人的名：一律不行。这一条挡住的是「读 B 的 storage」那条路
        assert!(!is_own_surface("qrfix", "pod-floattest"));
        // 浮舱那扇窗更不行 —— 程序舱把浮舱关了，用户会以为整个程序崩了
        assert!(!is_own_surface("floattest", "dock"));
        // 前缀相同不算：pod-qr 不该冒充得了 pod-qrfix
        assert!(!is_own_surface("qrfix", "pod-qr"));
        assert!(!is_own_surface("qr", "pod-qrfix"));
        // 空 slug 不该匹配上任何东西（清单读坏时 slug 可能是空的）
        assert!(!is_own_surface("", "pod-"));
        assert!(!is_own_surface("", ""));
    }

    #[test]
    fn errors_travel_in_the_envelope_not_the_status_code() {
        // 桥那边只有一条成功路径。要是失败走 HTTP 状态码，每个调用点都得写两遍错误处理，
        // 而漏写的那一半会表现为「promise 静默 reject」。
        let r = json_result(Err("permission_denied: 没申请".into()));
        assert_eq!(r.status(), 200);
        let v: serde_json::Value = serde_json::from_slice(r.body()).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"]
            .as_str()
            .unwrap()
            .starts_with("permission_denied"));
    }
}
