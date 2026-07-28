//! qrfix 程序舱的无头验收。
//!
//! 放在 podapp-qr 而不是 podapp-runtime：这个程序舱要用宿主注册的 `qr.*`，
//! 而运行时自己的测试**不该**装它 —— 那样就分不清「核心自带」和「宿主加装」了，
//! 而分不清正是可插拔要避免的。

use podapp_runtime::{Capabilities, HeadlessHost, Invocation};
use serde_json::{json, Value};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn have_node() -> bool {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::var("PATH").ok().is_some_and(|p| {
        let sep = if cfg!(windows) { ';' } else { ':' };
        p.split(sep)
            .any(|d| !d.is_empty() && std::path::Path::new(d).join(exe).exists())
    })
}

/// 造一张有底色的“海报”。纯白会让二维码贴上去边界不清，纯黑扫不出来 ——
/// 用中灰更接近真实海报，也更容易暴露对比度问题。
fn poster(path: &std::path::Path, w: u32, h: u32) {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        px[i * 4] = 120;
        px[i * 4 + 1] = 140;
        px[i * 4 + 2] = 190;
        px[i * 4 + 3] = 255;
    }
    let png = podapp_runtime::image::encode_png(&podapp_runtime::image::Img { w, h, px });
    std::fs::write(path, png).unwrap();
}

fn run(action: &str, input: Value) -> Result<Value, String> {
    podapp_runtime::headless::invoke(
        &Invocation::new(action, input),
        &HeadlessHost::new(),
        &Capabilities::builtin().with(podapp_qr::QrCapability),
        None,
    )
}

fn in_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let sb = std::env::temp_dir().join(format!("podapp-qrfix-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sb);
    std::fs::create_dir_all(&sb).unwrap();
    std::env::set_var("PODAPP_APPS_ROOT", &sb);
    std::env::set_var("PODAPP_ARTIFACTS_ROOT", sb.join("home"));
    podapp_runtime::install::install_from_path(&repo_root().join("pods/qrfix"), "test").unwrap();
    f(&sb);
    let _ = std::fs::remove_dir_all(&sb);
}

#[test]
fn a_pasted_code_is_actually_scannable() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("ok", |sb| {
        let p = sb.join("poster.png");
        poster(&p, 900, 1200);
        let url = "https://podapp.net/p/abc123";

        let out = run(
            "app.qrfix.code.replace",
            json!({ "poster": p.display().to_string(), "qr_text": url,
                    "at": { "x": 560, "y": 880, "w": 280, "h": 280 } }),
        )
        .unwrap_or_each_err();

        assert_eq!(out["verified"], true, "默认就该验证");
        // 这一条是整个程序舱存在的理由：成品**真的**扫得出来，而且内容没错
        assert_eq!(out["scanned_text"], url);
        assert!(std::path::Path::new(out["artifact"]["path"].as_str().unwrap()).exists());
        assert!(
            !out.to_string().contains("iVBORw0"),
            "返回值里混进了 PNG base64"
        );
    });
}

#[test]
fn a_code_too_small_to_scan_is_refused_rather_than_emitted() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("small", |sb| {
        let p = sb.join("poster.png");
        poster(&p, 400, 400);
        // 一段长文本塞进很小的方块 —— 模块会小到扫不出来
        let long = "https://podapp.net/very/long/path?".to_string() + &"x=1&".repeat(40);
        let e = run(
            "app.qrfix.code.replace",
            json!({ "poster": p.display().to_string(), "qr_text": long,
                    "at": { "x": 10, "y": 10, "w": 24, "h": 24 } }),
        )
        .unwrap_err();
        // 产一张扫不出来的图比不产出更坏 —— 用户会拿去印
        assert!(e.contains("扫不出来") || e.contains("编不成"), "实际: {e}");
    });
}

#[test]
fn pasting_outside_the_poster_is_refused_with_the_numbers() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("oob", |sb| {
        let p = sb.join("poster.png");
        poster(&p, 300, 300);
        let e = run(
            "app.qrfix.code.replace",
            json!({ "poster": p.display().to_string(), "qr_text": "x",
                    "at": { "x": 280, "y": 280, "w": 100, "h": 100 } }),
        )
        .unwrap_err();
        assert!(e.contains("超出海报"), "实际: {e}");
        assert!(e.contains("300"), "报错该带上实际尺寸，好让人照着改: {e}");
    });
}

#[test]
fn asking_for_neither_a_text_nor_an_image_says_what_to_give() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("none", |sb| {
        let p = sb.join("poster.png");
        poster(&p, 300, 300);
        let e = run(
            "app.qrfix.code.replace",
            json!({ "poster": p.display().to_string(), "at": { "x": 10, "y": 10, "w": 100, "h": 100 } }),
        )
        .unwrap_err();
        assert!(e.contains("qr_text") && e.contains("qr_image"), "实际: {e}");
    });
}

/// 小工具：失败时把错误原文带出来，省得只看到 unwrap panic
trait OrErr {
    fn unwrap_or_each_err(self) -> Value;
}
impl OrErr for Result<Value, String> {
    fn unwrap_or_each_err(self) -> Value {
        self.unwrap_or_else(|e| panic!("动作失败: {e}"))
    }
}
