//! 从**程序舱那一侧**走完整条链：清单申报 → 桥 → 权限闸 → 宿主动作 → 产物。
//!
//! 单测能证明「打包函数会打包」，证明不了「九宫格真调得到它」。中间任何一环
//! （清单少写一条、桥的名字对不上、权限闸拦错了）都会让功能在真机上不可用，
//! 而单测照样全绿。
//!
//! 需要本机有 Node（无头 runner 靠它）。没有就跳过 —— 但**明说跳过了**，
//! 不假装通过。

use podapp_runtime::headless::HeadlessHost;
use serde_json::{json, Value};
use std::path::PathBuf;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_sandbox(tag: &str, f: impl FnOnce(&std::path::Path)) {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let sandbox =
        std::env::temp_dir().join(format!("podapp-zipchain-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(&sandbox).unwrap();
    std::env::set_var("PODAPP_APPS_ROOT", &sandbox);
    std::env::set_var("PODAPP_ARTIFACTS_ROOT", sandbox.join("home"));
    f(&sandbox);
    std::env::remove_var("PODAPP_APPS_ROOT");
    std::env::remove_var("PODAPP_ARTIFACTS_ROOT");
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

fn make_png(path: &std::path::Path, w: u32, h: u32) {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let v = 255 - ((x * 128 / w) + (y * 127 / h)) as u8;
            px[i] = v;
            px[i + 1] = v;
            px[i + 2] = v;
            px[i + 3] = 255;
        }
    }
    std::fs::write(
        path,
        podapp_runtime::image::encode_png(&podapp_runtime::image::Img { w, h, px }),
    )
    .unwrap();
}

/// 宿主接上动作总线 —— 和浮舱 `host.rs` 里那一行是**同一个函数**。
/// 这里不许自己再拼一个「差不多的」实现，否则测的就不是会被交付的东西。
fn host_with_zip() -> HeadlessHost {
    HeadlessHost::with_host_actions(|id, input| podapp_zip::host_action(id, input))
}

#[test]
fn nine_grid_can_reach_the_zip_host_action() {
    if !have_node() {
        println!("跳过：本机没有 Node，无头 runner 跑不起来（这是环境缺失，不是通过）");
        return;
    }
    in_sandbox("ok", |sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/nine-grid"), "test")
            .unwrap_or_else(|e| panic!("装不上 nine-grid: {e}"));

        let img = sandbox.join("src.png");
        make_png(&img, 900, 600);

        let out: Value = podapp_runtime::headless::run_action_with(
            "app.nine-grid.image.split",
            json!({ "image": img.display().to_string(), "rows": 3, "cols": 3, "zip": true }),
            &host_with_zip(),
        )
        .unwrap_or_else(|e| panic!("无头执行失败: {e}"));

        assert_eq!(out["count"], 9);
        let zip = &out["zip"];
        assert!(!zip.is_null(), "传了 zip:true 却没拿到打包产物");

        // 交的是引用不是字节 —— 无头调用方拿到几 MB base64 是白烧上下文
        let path = zip["path"].as_str().expect("打包产物没有 path");
        assert!(path.ends_with(".zip"), "后缀应为 zip，实际 {path}");
        assert!(!out.to_string().contains("iVBORw0"), "返回值里混进了内容");

        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let eocd = bytes.len() - 22;
        assert_eq!(
            u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]),
            9,
            "九张切片应该一张不少地进包"
        );
        // 名字补零到两位，解压后按文件名排序就是原来的行列顺序
        for name in ["01-01.png", "01-02.png", "03-03.png"] {
            assert!(
                bytes.windows(name.len()).any(|w| w == name.as_bytes()),
                "包里缺 {name}"
            );
        }
    });
}

/// 不传 `zip` 时不该凭空多出一个产物 —— 打包是**额外要的**，不是默认行为。
#[test]
fn without_the_flag_nothing_is_packed() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("off", |sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/nine-grid"), "test")
            .unwrap();
        let img = sandbox.join("src.png");
        make_png(&img, 300, 300);

        let out: Value = podapp_runtime::headless::run_action_with(
            "app.nine-grid.image.split",
            json!({ "image": img.display().to_string(), "rows": 2, "cols": 2 }),
            &host_with_zip(),
        )
        .unwrap();

        assert_eq!(out["count"], 4);
        assert!(out["zip"].is_null(), "没要打包却打了");
        assert!(
            podapp_runtime::artifacts::list()
                .iter()
                .all(|a| a.kind != "archive"),
            "收件箱里不该出现 archive"
        );
    });
}

/// 没申报就调不动 —— 而且拦截发生在**面之下**，宿主接没接这个动作都一样。
///
/// 两个程序舱走同一条 `rpc` 入口、同一份宿主，唯一的差别是清单里申报没申报。
/// 一边通一边拒，才说明闸是按清单判的，而不是碰巧因为别的原因失败。
#[test]
fn the_gate_follows_the_manifest_not_the_host() {
    in_sandbox("perm", |_| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/memo"), "test").unwrap();
        podapp_runtime::install::install_from_path(&repo_root().join("pods/nine-grid"), "test")
            .unwrap();

        // 前提：备忘贴没申报任何宿主动作。它哪天申报了，这条测试要换一个 Pod，
        // 而不是删掉 —— 所以把前提也断言出来。
        assert!(
            podapp_runtime::manifest::permissions("org.podapp.productivity.memo")
                .expect("读不到备忘贴的权限")
                .host_actions
                .is_empty()
        );

        let host = host_with_zip();
        let call = |pod: &str| {
            podapp_runtime::headless::rpc(
                pod,
                "action",
                &json!({ "id": "host.zip.pack", "input": { "artifacts": ["art_whatever"] } }),
                &host,
            )
        };

        let denied = call("org.podapp.productivity.memo").unwrap_err();
        assert!(
            denied.starts_with("permission_denied"),
            "没申报的程序舱应被闸住，实际错误是：{denied}"
        );

        // 申报了的那个要能过闸。它会因为「产物不存在」失败 —— 那正说明
        // **已经走到宿主动作里面了**，闸没拦它。
        let passed = call("org.podapp.image.nine-grid").unwrap_err();
        assert!(
            passed.starts_with("not_found"),
            "申报过的程序舱不该被闸住，实际错误是：{passed}"
        );
    });
}
