//! `podapp-run` —— 泊舟的命令行面。
//!
//! # 为什么不做定时器
//!
//! 「每天早上出一份报告」这类需求需要的是**被调度**，不是自带调度器。机器上已经有
//! 调度器了：Windows 任务计划、cron、GitHub Actions、以及 AI agent 自己的定时任务。
//!
//! 自带一个意味着：浮舱必须常驻（关了就不跑）、要自己处理错过的时间点、要自己存
//! 任务表、重启后要恢复 —— 全是别人已经做好而且做得更好的事。
//!
//! **所以泊舟做的是「能被任何调度器调用的一个面」。** 定时那半交给系统：
//!
//! ```powershell
//! # 每天 8:00 出一份报告
//! schtasks /create /tn "podapp-morning" /sc daily /st 08:00 ^
//!   /tr "\"C:\\...\\podapp-run.exe\" flow C:\\...\\morning.flow.json --json"
//! ```
//!
//! 这也和「AI agent 的搭子」是同一件事：**它们调我们，不是我们调它们。**
//!
//! # 输出约定（给机器读）
//!
//! - **stdout 只有结果**，日志和进度一律 stderr —— 混在一起会让 `--json` 的输出
//!   在出问题的那天变成不可解析的东西，而那正是最需要它可解析的一天
//! - `--json` 给稳定形状；不带就给人看的几行字
//! - 退出码：`0` 成功 / `1` 用法或输入不对 / `2` 动作或流程失败 /
//!   `3` **停在等确认**（调度器据此知道「需要人」，而不是当成失败重试）
//! - 不带颜色、不带 spinner：非 TTY 下那些是垃圾字符

use serde_json::{json, Value};

const USAGE: &str = "\
podapp-run —— 泊舟的命令行面

  podapp-run install <目录或.pod> [--json]    装一个程序舱
  podapp-run actions [--json]                 列出能无头跑的动作
  podapp-run run <action-id> [--input <json>] [--json]
  podapp-run check <flow.json> [--json]       只验流程，不跑
  podapp-run flow <flow.json> [--from N] [--json]

退出码：0 成功 / 1 用法或输入不对 / 2 执行失败 / 3 停在等确认
";

/// 退出码。数字的含义写进 USAGE 和文档，调度器要靠它分支。
const EXIT_USAGE: i32 = 1;
const EXIT_FAILED: i32 = 2;
const EXIT_NEEDS_CONFIRM: i32 = 3;

struct Args {
    json: bool,
    rest: Vec<String>,
    input: Option<String>,
    from: usize,
}

fn parse_args() -> Args {
    let mut a = Args {
        json: false,
        rest: vec![],
        input: None,
        from: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--json" => a.json = true,
            "--input" => a.input = it.next(),
            "--from" => a.from = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            other => a.rest.push(other.to_string()),
        }
    }
    a
}

/// 出结果。**只有这一个函数往 stdout 写。**
fn emit(json_mode: bool, ok: bool, human: &str, data: Value) -> ! {
    if json_mode {
        println!("{}", json!({ "ok": ok, "data": data }));
    } else {
        println!("{human}");
    }
    std::process::exit(if ok { 0 } else { EXIT_FAILED });
}

/// 出错。人话走 stderr，`--json` 时结构化的那份也走 stdout ——
/// 调度器要么读退出码，要么读 JSON，两条都得能用。
fn die(json_mode: bool, code: i32, msg: &str) -> ! {
    if json_mode {
        println!("{}", json!({ "ok": false, "error": msg }));
    }
    eprintln!("{msg}");
    std::process::exit(code)
}

fn read_flow(path: &str, json_mode: bool) -> podapp_flow::Flow {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| die(json_mode, EXIT_USAGE, &format!("读不了 {path}：{e}")));
    let v: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| die(json_mode, EXIT_USAGE, &format!("{path} 不是能读的 JSON：{e}")));
    podapp_flow::parse(&v).unwrap_or_else(|e| die(json_mode, EXIT_USAGE, &e))
}

fn main() {
    let a = parse_args();
    let _ = podapp_runtime::init(podapp_runtime::HostProfile::podapp(env!(
        "CARGO_PKG_VERSION"
    )));
    let caps = podapp_host::capabilities();
    let host = podapp_host::headless_host();

    match a.rest.first().map(String::as_str) {
        // 无头机器上没有浮舱可以拖，所以 CLI 这个面必须能自己装 ——
        // 少了它，「在服务器上定时跑一条流程」这件事根本起不来
        Some("install") => {
            let Some(path) = a.rest.get(1) else {
                die(a.json, EXIT_USAGE, USAGE);
            };
            match podapp_runtime::install::install_from_path(std::path::Path::new(path), "cli") {
                Ok(info) => emit(
                    a.json,
                    true,
                    &format!("已装 {} v{}", info.name, info.version),
                    json!({ "id": info.id, "name": info.name, "version": info.version }),
                ),
                Err(e) => die(a.json, EXIT_FAILED, &e),
            }
        }

        Some("actions") => {
            let specs = podapp_runtime::manifest::action_specs();
            let rows: Vec<Value> = specs
                .iter()
                .map(|s| {
                    json!({ "id": s.id, "title": s.title, "effect": s.effect,
                            "confirm": s.confirmation != "never" })
                })
                .collect();
            let human = specs
                .iter()
                .map(|s| format!("{}\t{}", s.id, s.title))
                .collect::<Vec<_>>()
                .join("\n");
            emit(a.json, true, &human, json!({ "actions": rows }));
        }

        Some("run") => {
            let Some(id) = a.rest.get(1) else {
                die(a.json, EXIT_USAGE, USAGE);
            };
            let input: Value = match &a.input {
                None => json!({}),
                Some(s) => serde_json::from_str(s).unwrap_or_else(|e| {
                    die(a.json, EXIT_USAGE, &format!("--input 不是 JSON：{e}"))
                }),
            };
            let inv = podapp_runtime::Invocation::new(id, input);
            match podapp_runtime::headless::invoke(&inv, &host, &caps, None) {
                Ok(v) => {
                    // 人话优先给 message；产物只给路径 —— 把内容打到 stdout
                    // 会让「结果」和「数据」混成一坨
                    let human = v
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("完成")
                        .to_string();
                    emit(a.json, true, &human, v);
                }
                Err(e) => die(a.json, EXIT_FAILED, &e),
            }
        }

        Some("check") => {
            let Some(path) = a.rest.get(1) else {
                die(a.json, EXIT_USAGE, USAGE);
            };
            let flow = read_flow(path, a.json);
            let problems = podapp_flow::check(&flow);
            if problems.is_empty() {
                emit(
                    a.json,
                    true,
                    &format!("{} · {} 步，可以跑", flow.name, flow.steps.len()),
                    json!({ "name": flow.name, "steps": flow.steps.len(), "problems": [] }),
                );
            }
            // 验不过是**输入的问题**，不是执行失败 —— 用 1 而不是 2，
            // 好让调度器区分「我给错了」和「跑坏了」
            if a.json {
                println!("{}", json!({ "ok": false, "data": { "problems": problems } }));
            }
            for p in &problems {
                eprintln!("{p}");
            }
            std::process::exit(EXIT_USAGE);
        }

        Some("flow") => {
            let Some(path) = a.rest.get(1) else {
                die(a.json, EXIT_USAGE, USAGE);
            };
            let flow = read_flow(path, a.json);
            let problems = podapp_flow::check(&flow);
            if !problems.is_empty() {
                die(a.json, EXIT_USAGE, &problems.join("\n"));
            }
            let outcome = podapp_flow::run(&flow, None, a.from, vec![], &host, &caps);
            let data = outcome.to_json();
            match &outcome {
                podapp_flow::Outcome::Done { results } => emit(
                    a.json,
                    true,
                    &format!("跑完了 · {} 步有结果", results.len()),
                    data,
                ),
                podapp_flow::Outcome::NeedsConfirm { step, title, .. } => {
                    // **不在无人值守的地方替人点头。** 明确用 3 退出，
                    // 让调度器知道「需要人」而不是当成失败去重试 ——
                    // 重试一个等确认的流程只会每天重跑前面几步
                    if a.json {
                        println!("{}", json!({ "ok": false, "data": data }));
                    }
                    eprintln!(
                        "停在第 {} 步「{title}」等确认。命令行不代替人点头 —— \
在浮舱里确认，或用 --from {} 明确跳过它。",
                        step + 1,
                        step + 1
                    );
                    std::process::exit(EXIT_NEEDS_CONFIRM);
                }
                podapp_flow::Outcome::Failed { step, error, .. } => {
                    if a.json {
                        println!("{}", json!({ "ok": false, "data": data }));
                    }
                    eprintln!("第 {} 步失败：{error}", step + 1);
                    std::process::exit(EXIT_FAILED);
                }
            }
        }

        _ => {
            // 用法写 stderr：`podapp-run | jq` 这种写法下，用法文本进 stdout
            // 会让 jq 报一个跟真实原因无关的解析错
            eprintln!("{USAGE}");
            std::process::exit(EXIT_USAGE);
        }
    }
}
