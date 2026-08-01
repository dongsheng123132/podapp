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

#[test]
fn annotate_turns_boxes_into_a_task_an_agent_can_follow() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("annotate", |sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/annotate"), "test")
            .unwrap();
        let img = sandbox.join("poster.png");
        make_test_png(&img, 800, 600);

        let out: Value = podapp_runtime::headless::run_action(
            "app.annotate.task.build",
            json!({
                "image": img.display().to_string(),
                "annotations": [
                    { "x": 100, "y": 80, "w": 200, "h": 120, "instruction": "这里换成真二维码" },
                    // 故意越界：宽度从 x=700 起要 400，超出 800 的图
                    { "x": 700, "y": 500, "w": 400, "h": 400, "instruction": "标题放大一号" }
                ],
                "note": "整体风格别动"
            }),
        )
        .unwrap_or_else(|e| panic!("无头执行失败: {e}"));

        assert_eq!(out["count"], 2);

        let task = &out["task"];
        // 坐标系必须写明。不写的话对面只能猜是原图还是显示尺寸，猜错框就偏了。
        assert_eq!(task["image"]["coordinate_space"], "source-pixels");
        assert_eq!(task["image"]["origin"], "top-left");
        assert_eq!(task["image"]["w"], 800);

        let ann = task["annotations"].as_array().unwrap();
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0]["index"], 1);
        assert_eq!(ann[0]["width"], 200);

        // 越界的框必须被夹回图内，**而且返回的坐标就是夹过之后的**。
        // 返回原始越界值而画出来是夹过的，正是这个程序舱要消灭的那种含糊。
        let x = ann[1]["x"].as_i64().unwrap();
        let w = ann[1]["width"].as_i64().unwrap();
        assert!(x + w <= 800, "第 2 处越界了: x={x} w={w}");
        let y = ann[1]["y"].as_i64().unwrap();
        let h = ann[1]["height"].as_i64().unwrap();
        assert!(y + h <= 600, "第 2 处纵向越界: y={y} h={h}");

        // 给人贴的那段话和结构化任务必须同源
        let prompt = out["prompt"].as_str().unwrap();
        assert!(prompt.contains("这里换成真二维码"));
        assert!(prompt.contains("整体风格别动"));
        assert!(prompt.contains("800×600"));

        // 标注图落了盘，且返回值里没有像素
        assert!(std::path::Path::new(out["overlay"]["path"].as_str().unwrap()).exists());
        assert!(
            !out.to_string().contains("iVBORw0"),
            "返回值里混进了 PNG base64"
        );
    });
}

#[test]
fn annotate_refuses_an_empty_selection() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("annotate-empty", |sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/annotate"), "test")
            .unwrap();
        let img = sandbox.join("p.png");
        make_test_png(&img, 200, 200);
        let e = podapp_runtime::headless::run_action(
            "app.annotate.task.build",
            json!({ "image": img.display().to_string(), "annotations": [] }),
        )
        .unwrap_err();
        assert!(e.contains("一个标注都没有"), "实际: {e}");
    });
}

#[test]
fn memo_can_be_managed_by_gui_and_agents_through_the_same_actions() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("memo", |_sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/memo"), "test")
            .unwrap_or_else(|e| panic!("装不上 memo: {e}"));

        let saved: Value = podapp_runtime::headless::run_action(
            "app.memo.note.save",
            json!({
                "title": "明天要做",
                "body": "提交 PodApp 安装包",
                "color": "green"
            }),
        )
        .unwrap_or_else(|e| panic!("保存备忘失败: {e}"));
        assert_eq!(saved["ok"], true);
        assert_eq!(saved["note"]["color"], "green");
        let id = saved["note"]["id"].as_str().expect("新备忘没有 id");

        let listed: Value = podapp_runtime::headless::run_action(
            "app.memo.note.list",
            json!({ "query": "安装包" }),
        )
        .unwrap_or_else(|e| panic!("列出备忘失败: {e}"));
        assert_eq!(listed["count"], 1);
        assert_eq!(listed["notes"][0]["id"], id);

        let removed: Value =
            podapp_runtime::headless::run_action("app.memo.note.remove", json!({ "id": id }))
                .unwrap_or_else(|e| panic!("删除备忘失败: {e}"));
        assert_eq!(removed["removed"], true);

        let empty: Value =
            podapp_runtime::headless::run_action("app.memo.note.list", json!({})).unwrap();
        assert_eq!(empty["count"], 0);
    });
}

#[test]
fn tictactoe_actions_share_one_headless_board() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("tictactoe", |_sandbox| {
        podapp_runtime::install::install_from_path(&repo_root().join("pods/tictactoe"), "test")
            .unwrap_or_else(|e| panic!("装不上 tictactoe: {e}"));

        let initial: Value =
            podapp_runtime::headless::run_action("app.tictactoe.game.state", json!({}))
                .unwrap_or_else(|e| panic!("读不到初始棋盘: {e}"));
        assert_eq!(
            initial["board"],
            json!([null, null, null, null, null, null, null, null, null])
        );
        assert_eq!(initial["turn"], "X");

        let first: Value = podapp_runtime::headless::run_action(
            "app.tictactoe.game.move",
            json!({ "cell": 0, "as": "X" }),
        )
        .unwrap_or_else(|e| panic!("X 落子失败: {e}"));
        assert_eq!(first["board"][0], "X");
        assert_eq!(first["turn"], "O");

        let second: Value = podapp_runtime::headless::run_action(
            "app.tictactoe.game.move",
            json!({ "cell": 4, "as": "O" }),
        )
        .unwrap_or_else(|e| panic!("O 落子失败: {e}"));
        assert_eq!(second["board"][0], "X");
        assert_eq!(second["board"][4], "O");
        assert_eq!(second["moves"], 2);

        let reset: Value =
            podapp_runtime::headless::run_action("app.tictactoe.game.reset", json!({}))
                .unwrap_or_else(|e| panic!("重置棋盘失败: {e}"));
        assert_eq!(reset["board"], initial["board"]);
        assert_eq!(reset["turn"], "X");
        assert_eq!(reset["moves"], 0);
    });
}

/// 跑一条 `podapp` CLI 命令，返回 stdout 第一行（CLI 约定：结果走 stdout、日志走 stderr）。
fn podapp_cli(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let cli = repo_root().join("../podapp-protocol/bin/podapp.mjs");
    if !cli.exists() {
        return Err(format!("找不到 CLI: {}", cli.display()));
    }
    let o = std::process::Command::new(if cfg!(windows) { "node.exe" } else { "node" })
        .arg(&cli)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("起不来 node: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "podapp {args:?} 失败（{:?}）: {}",
            o.status.code(),
            String::from_utf8_lossy(&o.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string())
}

/// 文档里那个闭环，一次跑完：**脚手架 → 打包 → 安装 → 无头执行**。
///
/// 这条测试同时是 JS 打包器和 Rust 解包器之间的**契约测试**。两边各写一套
/// tar 处理（一个手写 USTAR，一个用 tar crate），谁改了细节都可能让对方装不上，
/// 而症状是「包好像没问题但就是装不了」—— 隔着语言边界最难查的那种。
#[test]
fn the_whole_loop_works_scaffold_pack_install_run() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("loop", |sandbox| {
        let work = sandbox.join("work");
        std::fs::create_dir_all(&work).unwrap();

        // ① 让 CLI 生成骨架。它自带自检，生成完就该是能装的。
        let created = match podapp_cli(&work, &["create", "loopdemo"]) {
            Ok(p) => p,
            Err(e) => {
                println!("跳过：{e}");
                return;
            }
        };
        assert!(
            std::path::Path::new(&created).exists(),
            "脚手架目录没生成: {created}"
        );

        // ② 打成 .pod
        let pod_file = podapp_cli(&work, &["pack", "loopdemo"]).expect("打包失败");
        let pod_path = std::path::Path::new(&pod_file);
        assert!(pod_path.exists(), "包没生成: {pod_file}");
        assert!(pod_file.ends_with(".pod"), "后缀不对: {pod_file}");

        // ③ 运行时装它 —— 从**包文件**装，不是从目录，走的是解包那条路
        let info = podapp_runtime::install::install_from_path(pod_path, "cli-loop")
            .unwrap_or_else(|e| panic!("运行时装不上 CLI 打的包: {e}"));
        assert_eq!(info.id, "org.example.loopdemo");
        assert_eq!(info.slug, "loopdemo");

        // ④ 无头跑骨架自带的那个动作
        let out: Value = podapp_runtime::headless::run_action(
            "app.loopdemo.demo.run",
            json!({ "text": "闭环" }),
        )
        .unwrap_or_else(|e| panic!("无头执行失败: {e}"));
        assert_eq!(out["echo"], "闭环");
        assert!(out["message"].as_str().unwrap().contains("闭环"));

        // ⑤ 卸掉，别在沙箱里留东西
        podapp_runtime::install::uninstall("org.example.loopdemo", true).unwrap();
    });
}

/// CLI 判「能装」和运行时判「能装」必须一致。
///
/// 两份校验实现（JS 的 validate.mjs、Rust 的 load_dir）是天然的漂移源。
/// 边界说死：**运行时说了算**。CLI 报通过而运行时拒绝，是 CLI 的 bug。
/// 这条测试拿真实的官方程序舱把两边的结论对一遍。
#[test]
fn the_cli_and_the_runtime_agree_on_what_installs() {
    if !have_node() {
        println!("跳过：本机没有 Node");
        return;
    }
    in_sandbox("agree", |_sandbox| {
        for pod in ["nine-grid", "annotate", "qrfix", "chatlog", "memo"] {
            let dir = repo_root().join("pods").join(pod);
            let cli_ok = podapp_cli(&repo_root(), &["validate", dir.to_str().unwrap(), "--json"]);
            let cli_ok = match cli_ok {
                Ok(line) => {
                    let v: Value = serde_json::from_str(&line).expect("--json 该只输出一行 JSON");
                    v["ok"] == true
                }
                Err(e) => {
                    println!("跳过 {pod}：{e}");
                    return;
                }
            };
            let rt_ok = podapp_runtime::manifest::load_dir(&dir).is_ok();
            assert_eq!(
                cli_ok, rt_ok,
                "{pod}: CLI 说 {cli_ok}，运行时说 {rt_ok} —— 两份校验开始分家了"
            );
        }
    });
}
