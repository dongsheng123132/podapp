//! 资源服务 —— 把 `<pod-id>/<web.root>/<rel>` 喂给 WebView。
//!
//! 清单校验失败时返回**可读的红字页面**而不是白屏。白屏对用户是「坏了但不知道为什么」，
//! 对作者是「我改了什么？」—— 一行错误信息省掉的来回可能是几十分钟。

use crate::manifest::{load_dir, resolve_dir};

pub struct Served {
    pub status: u16,
    pub mime: &'static str,
    pub body: Vec<u8>,
    /// 开发态不缓存，保存刷新即见
    pub no_store: bool,
}

fn mime_of(p: &str) -> &'static str {
    match p.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "md" | "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn err_page(status: u16, msg: &str) -> Served {
    Served {
        status,
        mime: "text/html; charset=utf-8",
        body: format!(
            "<!doctype html><meta charset=utf-8><body style=\"font:14px/1.6 system-ui;background:#14161a;color:#fca5a5;padding:32px\">\
             <h2 style=\"margin:0 0 8px\">程序舱打不开</h2><pre style=\"white-space:pre-wrap;color:#e6e8ec\">{}</pre></body>",
            msg.replace('<', "&lt;")
        )
        .into_bytes(),
        no_store: true,
    }
}

/// 服务一个程序舱的静态资源。
///
/// `rel` 为空 = 入口页；`__icon` = 图标（走独立虚路径，前端 `<img src=".../__icon">`）。
pub fn serve(pod_id: &str, rel: &str) -> Served {
    let Some(dir) = resolve_dir(pod_id) else {
        return err_page(404, &format!("没装这个程序舱: {pod_id}"));
    };
    let (m, _) = match load_dir(&dir) {
        Ok(v) => v,
        Err(e) => return err_page(500, &e),
    };
    let dev = crate::manifest::is_dev_dir(&dir);

    if rel == "__icon" {
        if m.ui.icon.starts_with("lucide:") {
            return err_page(404, "该程序舱使用内置图标");
        }
        return match crate::safe_join(&dir, &m.ui.icon).and_then(|p| std::fs::read(p).ok()) {
            Some(b) => Served { status: 200, mime: mime_of(&m.ui.icon), body: b, no_store: dev },
            None => err_page(404, "图标读不到"),
        };
    }

    if m.package.kind != "web" {
        return err_page(400, "这个程序舱不是 web 形态，没有可服务的页面");
    }
    let w = m.package.web.clone().unwrap_or_default();
    let Some(root) = crate::safe_join(&dir, &w.root) else {
        return err_page(500, "package.web.root 路径非法");
    };
    let rel = if rel.is_empty() { w.entry.as_str() } else { rel };
    // 这道 safe_join 是路径穿越的唯一闸门 —— 删了它，`../../../.podapp/device.json` 就出去了
    let Some(file) = crate::safe_join(&root, rel) else {
        return err_page(403, "路径非法");
    };
    match std::fs::read(&file) {
        Ok(b) => Served { status: 200, mime: mime_of(rel), body: b, no_store: dev },
        Err(_) => err_page(404, &format!("找不到 {rel}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_covers_what_pods_actually_ship() {
        assert_eq!(mime_of("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_of("actions.mjs"), "text/javascript; charset=utf-8");
        assert_eq!(mime_of("icon.PNG"), "image/png", "扩展名大小写不该影响");
        assert_eq!(mime_of("noext"), "application/octet-stream");
    }

    #[test]
    fn err_page_escapes_markup() {
        let p = err_page(500, "<script>alert(1)</script>");
        let s = String::from_utf8(p.body).unwrap();
        assert!(!s.contains("<script>alert"), "错误信息必须转义，否则错误页自己是注入点");
    }
}
