//! `podapp://` 协议处理器 —— 把运行时接到 WebView 上。
//!
//! 四条路径，全都收敛到 `podapp-runtime`，这里不做第二份实现：
//!
//! | 路径 | 干什么 |
//! |---|---|
//! | `/__podapp/bridge.js` | 桥脚本 |
//! | `/app/<pod-id>/<rel>` | 程序舱静态资源（入口页会被注入桥） |
//! | `/rpc/<pod-id>/<verb>` | 能力调用（POST） |
//! | `/artifact/<id>` | 产物字节 |
//!
//! **CSP 由 [`podapp_runtime::perms::csp_for`] 逐个程序舱下发**，不在这里写死。
//! 写死就意味着「这个程序舱申请了哪些网络源」这件事有两份定义。

use podapp_runtime::{bridge, manifest, perms, serve};
use tauri::http::{Request, Response};

/// 从 `/app/<pod-id>/<rel...>` 里拆出 pod-id 和相对路径。
fn split_pod_path<'a>(path: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = path.strip_prefix(prefix)?;
    match rest.split_once('/') {
        Some((id, rel)) => Some((id, rel)),
        None => Some((rest, "")),
    }
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
        let args: serde_json::Value =
            serde_json::from_slice(req.body()).unwrap_or(serde_json::Value::Null);
        let host = crate::host::DockHost;
        // 走浮舱那一份能力集 —— 界面这条路和无头那条路必须看到同样的动词
        return json_result(crate::rpc_with_dock_capabilities(pod_id, verb, &args, &host));
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

    if let Some((pod_id, rel)) = split_pod_path(&path, "/app/") {
        let served = serve::serve(pod_id, rel);
        let is_entry = served.status == 200 && served.mime.starts_with("text/html");
        // 桥只注入 HTML 文档。注入到 js/png 上会把文件内容弄坏，
        // 而「图片打不开」根本不会让人联想到桥。
        let body = if is_entry { bridge::inject(&served.body, pod_id) } else { served.body };

        let csp = manifest::permissions(pod_id).map(|p| perms::csp_for(&p)).unwrap_or_default();
        let mut b = Response::builder().status(served.status).header("content-type", served.mime);
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
        assert_eq!(split_pod_path("/app/org.x.y/web/a.js", "/app/"), Some(("org.x.y", "web/a.js")));
        // 没有尾部路径 = 入口页
        assert_eq!(split_pod_path("/app/org.x.y", "/app/"), Some(("org.x.y", "")));
        assert_eq!(split_pod_path("/app/org.x.y/", "/app/"), Some(("org.x.y", "")));
        assert_eq!(split_pod_path("/rpc/org.x.y/image.decode", "/rpc/"), Some(("org.x.y", "image.decode")));
        assert_eq!(split_pod_path("/other", "/app/"), None);
    }

    #[test]
    fn errors_travel_in_the_envelope_not_the_status_code() {
        // 桥那边只有一条成功路径。要是失败走 HTTP 状态码，每个调用点都得写两遍错误处理，
        // 而漏写的那一半会表现为「promise 静默 reject」。
        let r = json_result(Err("permission_denied: 没申请".into()));
        assert_eq!(r.status(), 200);
        let v: serde_json::Value = serde_json::from_slice(r.body()).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().starts_with("permission_denied"));
    }
}
