//! 官方程序舱的**无头**验收 —— 真装、真跑、真验结果。
//!
//! 这条测试守的是标准的立身之本：程序舱的动作必须能在没有界面的情况下跑出正确结果。
//! 界面好不好看是另一回事；「AI 调得动」不是宣传语，是每次构建都要被证明一次的事。
//!
//! 需要本机有 Node（无头 runner 靠它）。没有就跳过 —— 但**会明确说跳过了**，
//! 不会假装通过。静默跳过的测试比没有测试更坏：它让人以为验过了。

use serde_json::{json, Value};
use std::path::PathBuf;

/// 把这些测试串起来跑。
///
/// 家目录是靠**进程级环境变量**顶掉的，而 `cargo test` 默认多线程并行 ——
/// 三个测试各设各的 `PODAPP_APPS_ROOT`，互相踩到的表现是
/// 「unknown_action」和「文件被占用」，看起来完全不像并发问题。
///
/// 不用 `--test-threads=1` 解决：需要特殊参数才能通过的测试是个陷阱，
/// 下一个人（或 CI）照常跑 `cargo test` 就会看到红的。锁在代码里，谁跑都对。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 开一个隔离沙箱跑一段测试。**绝不碰用户真实的 ~/.podapp**。
fn in_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let sandbox = std::env::temp_dir().join(format!("podapp-pods-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(&sandbox).unwrap();
    std::env::set_var("PODAPP_APPS_ROOT", &sandbox);
    std::env::set_var("PODAPP_ARTIFACTS_ROOT", sandbox.join("home"));
    f(&sandbox);
    let _ = std::fs::remove_dir_all(&sandbox);
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

/// 造一张可辨认的测试图：每个格子填不同的灰度，切完能验出「切对了哪一块」。
/// 写成真 PNG 文件而不是 data URL —— `image.decode` 两种都吃，用路径省掉 base64。
fn make_test_png(path: &std::path::Path, w: u32, h: u32) {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            // 左上角亮、右下角暗，任何一块切片都能从灰度判断出它原来在哪
            let v = 255 - ((x * 128 / w) + (y * 127 / h)) as u8;
            px[i] = v;
            px[i + 1] = v;
            px[i + 2] = v;
            px[i + 3] = 255;
        }
    }
    let png = podapp_runtime::image::encode_png(&podapp_runtime::image::Img { w, h, px });
    std::fs::write(path, png).unwrap();
}

#[test]
fn nine_grid_splits_correctly_with_no_ui() {
    if !have_node() {
        println!("跳过：本机没有 Node，无头 runner 跑不起来（这是环境缺失，不是通过）");
        return;
    }

    in_sandbox("split", |sandbox| {
        let src = repo_root().join("pods/nine-grid");
        let info = podapp_runtime::install::install_from_path(&src, "test")
            .unwrap_or_else(|e| panic!("装不上 nine-grid: {e}"));
        assert_eq!(info.id, "org.podapp.image.nine-grid");
        assert!(
            info.actions
                .contains(&"app.nine-grid.image.split".to_string()),
            "动作没并进总线: {:?}",
            info.actions
        );

        // 900×600 能被 3 整除，先验最干净的情形
        let img = sandbox.join("src.png");
        make_test_png(&img, 900, 600);

        let out: Value = podapp_runtime::headless::run_action(
            "app.nine-grid.image.split",
            json!({ "image": img.display().to_string(), "rows": 3, "cols": 3 }),
        )
        .unwrap_or_else(|e| panic!("无头执行失败: {e}"));

        assert_eq!(out["count"], 9);
        assert_eq!(out["cell"]["w"], 300);
        assert_eq!(out["cell"]["h"], 200);

        let tiles = out["tiles"].as_array().unwrap();
        assert_eq!(tiles.len(), 9);
        for t in tiles {
            // 产物必须是**引用**：有 id 和落盘路径
            let a = &t["artifact"];
            assert!(a["id"].as_str().is_some(), "产物没有 id: {t}");
            let p = a["path"].as_str().expect("产物没有 path");
            assert!(std::path::Path::new(p).exists(), "产物路径不存在: {p}");
        }

        // 返回值里**不许**夹带像素。无头调用方（Claude Code / MCP）拿到几 MB base64
        // 是一屏乱码 + 白烧上下文，而这条最容易在「顺手把 dataUrl 也返回去」时破功。
        let s = out.to_string();
        assert!(!s.contains("data:image"), "返回值里混进了 data URL");
        assert!(!s.contains("iVBORw0"), "返回值里混进了 PNG base64");
    });
}

#[test]
fn nine_grid_covers_every_pixel_when_the_size_is_not_divisible() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("odd", |sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/nine-grid"), "test")
            .unwrap();

        // 1001×701 除不尽 3。这才是真实图片的常态，而取整误差累加会让最后一列短掉几像素 ——
        // 拼回去之前根本看不出来，所以必须断言「九块加起来正好是整张」。
        let img = sandbox.join("odd.png");
        make_test_png(&img, 1001, 701);

        let out: Value = podapp_runtime::headless::run_action(
            "app.nine-grid.image.split",
            json!({ "image": img.display().to_string(), "rows": 3, "cols": 3 }),
        )
        .unwrap();

        let tiles = out["tiles"].as_array().unwrap();
        let width_of_row: i64 = tiles
            .iter()
            .filter(|t| t["row"] == 1)
            .map(|t| t["w"].as_i64().unwrap())
            .sum();
        let height_of_col: i64 = tiles
            .iter()
            .filter(|t| t["col"] == 1)
            .map(|t| t["h"].as_i64().unwrap())
            .sum();
        assert_eq!(
            width_of_row, 1001,
            "一行三块加起来不等于原图宽 —— 有像素被丢了"
        );
        assert_eq!(height_of_col, 701, "一列三块加起来不等于原图高");
    });
}

#[test]
fn a_gap_wider_than_the_image_is_refused_with_a_usable_message() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("gap", |sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/nine-grid"), "test")
            .unwrap();
        let img = sandbox.join("small.png");
        make_test_png(&img, 60, 60);

        let e = podapp_runtime::headless::run_action(
            "app.nine-grid.image.split",
            json!({ "image": img.display().to_string(), "rows": 3, "cols": 3, "gap": 100 }),
        )
        .unwrap_err();
        // 报错要能照着改，不能只说「失败」
        assert!(e.contains("至少需要"), "错误信息没给出可照做的下限: {e}");
    });
}
