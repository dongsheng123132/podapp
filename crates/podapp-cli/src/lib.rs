//! 把本机已装的命令行工具端给程序舱 —— 让终端工具有个界面。
//!
//! # 为什么这件事值得做
//!
//! 一堆最好用的工具只有终端界面，而它们缺的不是功能，是**一个能瞥一眼的面**。
//! 浮舱贴在 AI CLI 旁边，最该回答的问题是「它刚改了什么」—— 那是 `git status`
//! 和 `git diff`，一天要问几十次，每次都要切窗口敲一遍。
//!
//! 这也是浮舱和 AI 的正确关系：**它不替 AI 干活，它给 AI 的工具装个面**。
//!
//! # 这跟「不接 AI 能力」冲突吗——不冲突
//!
//! 红线挡的是**接入**：带 SDK、管密钥、背计费、引一条跟着上游改版的依赖。
//! 调用用户**自己装好、自己配好**的命令行工具不是这些中的任何一样：密钥是他的，
//! 计费是他的账号，我们一个依赖都不引。所以 `host.cli.codex` 这类将来也是合法的 ——
//! 那时浮舱是壳，AI 还是他的 AI。
//!
//! # 三条不肯让步的形状
//!
//! **1. 意图在程序舱，argv 在宿主。** 程序舱说「我要 status」，命令行怎么拼由这里决定。
//! 让程序舱自己传 argv 等于给它任意命令执行 —— `git` 光靠参数就能改配置、跑外部
//! diff 程序、走别名。**它表达意图，不表达命令。**
//!
//! **2. 有哪些程序由宿主定，不由程序舱定。** 程序舱在 `permissions.host_actions` 里
//! 申报 `host.cli.git`，装包时明明白白列给用户看；但申报一个这里没实现的（`host.cli.rm`）
//! 只会拿到 `capability_unavailable`。**白名单在宿主这一侧**，程序舱写什么都越不出去。
//!
//! **3. 不经 shell。** 只用 argv 数组，永远不拼字符串交给 `cmd` / `sh` ——
//! 没有 shell，就没有注入这个类别。
//!
//! # 为什么不进 `podapp-runtime`
//!
//! 跟 `podapp-qr` / `podapp-zip` 一样：可插拔能力，不是核心。宿主不想让程序舱碰
//! 子进程，删掉这个 crate 和 `host.rs` 里那一行分发就没了。**核心的依赖数不该因为它变化**，
//! 所以这里一个第三方依赖都不引 —— `std::process` 就够。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// 一条命令最多跑多久。
///
/// 子进程是会卡的东西，卡住的表现是「界面点了没反应」，而人不会想到是某个命令在等 IO。
/// 10 秒对本地 git 操作是很宽的上限：大仓库的 `status` 也就百毫秒级。
const TIMEOUT: Duration = Duration::from_secs(10);

/// 输出上限。超过就截断并**在结果里说明截断了** ——
/// 静默截断会让人拿着半份 diff 得出错误结论。
const MAX_OUTPUT: usize = 1024 * 1024;

/// 这个 crate 认得的宿主动作。
pub fn ids() -> &'static [&'static str] {
    &["host.cli.git"]
}

/// 宿主动作入口。
pub fn host_action(id: &str, input: Value) -> Result<Value, String> {
    match id {
        "host.cli.git" => git(&input),
        other => Err(format!("capability_unavailable: 没有宿主动作 {other}")),
    }
}

/// 跑一条 git 只读查询。
///
/// 只收 `op`，不收 argv。三个 op 都是只读的，而且都不触发 hook ——
/// `status` / `log` / `diff` 在 git 里不跑钩子，所以不需要额外去中和 `core.hooksPath`。
fn git(input: &Value) -> Result<Value, String> {
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    let cwd = checked_dir(input.get("cwd").and_then(Value::as_str).unwrap_or(""))?;
    // limit 只影响 log 条数。夹紧是因为「给个 100000」会让输出撞上截断上限，
    // 而那时人看到的是一份被切掉的日志
    let limit = input
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 200)
        .to_string();

    // argv **在这里拼**，程序舱碰不到。
    // `--no-pager` 是必须的：git 起了分页器就会等输入，表现是这条命令永远不返回。
    let mut args: Vec<String> = vec!["--no-pager".into()];
    match op {
        // porcelain=v1 是稳定的机器格式，专门承诺跨版本不变
        "status" => args.extend(["status", "--porcelain=v1", "--branch"].map(String::from)),
        "diff" => args.extend(["diff", "--stat", "--no-color"].map(String::from)),
        "log" => args.extend(
            [
                "log",
                "--no-color",
                "--pretty=format:%h\t%an\t%ad\t%s",
                "--date=short",
                "-n",
            ]
            .map(String::from)
            .into_iter()
            .chain([limit]),
        ),
        "" => return Err("invalid_input: 缺少 op（status / diff / log）".into()),
        other => return Err(format!("invalid_input: 不认得的 op：{other}")),
    }

    let out = run("git", &args, &cwd)?;
    Ok(json!({
        "op": op,
        "cwd": cwd.display().to_string(),
        "exit": out.exit,
        "stdout": out.stdout,
        "stderr": out.stderr,
        "truncated": out.truncated,
    }))
}

/// 校验并解析工作目录。
///
/// 必须是**已经存在的目录**。放行不存在的路径没有意义，而错误信息含糊
/// （「命令失败」）会让人去查 git 而不是查自己传的路径。
fn checked_dir(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() {
        return Err("invalid_input: 缺少 cwd".into());
    }
    let p = Path::new(raw);
    if !p.is_absolute() {
        // 相对路径会相对**宿主进程**的当前目录，那对程序舱毫无意义，
        // 而它偶然能跑通的那几次最误导人
        return Err("invalid_input: cwd 必须是绝对路径".into());
    }
    let real = std::fs::canonicalize(p).map_err(|e| format!("invalid_input: 打不开 cwd：{e}"))?;
    if !real.is_dir() {
        return Err("invalid_input: cwd 不是目录".into());
    }
    Ok(real)
}

struct Output {
    exit: i32,
    stdout: String,
    stderr: String,
    truncated: bool,
}

fn cap(mut s: String) -> (String, bool) {
    if s.len() <= MAX_OUTPUT {
        return (s, false);
    }
    // 按字符边界截，不然会切出半个 UTF-8 字符，而那会让 JSON 序列化失败 ——
    // 报错跟「输出太大」看不出关系
    let mut end = MAX_OUTPUT;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    (s, true)
}

/// 跑一个程序。**没有 shell**，argv 直接给操作系统。
fn run(program: &str, args: &[String], cwd: &Path) -> Result<Output, String> {
    let child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        // 关掉 stdin：程序要是想问点什么，立刻拿到 EOF 而不是永远等着。
        // 这条比超时更重要 —— 超时会让人等满 10 秒才知道出事了
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // 中和几个会改变行为的环境变量。用户的环境不是我们的问题，
        // 但**同一次调用在不同机器上该给同样的结果**
        .env("GIT_PAGER", "cat")
        .env("GIT_EXTERNAL_DIFF", "")
        .env("GIT_TERMINAL_PROMPT", "0")
        // 不去抢索引锁：只读查询不该跟用户自己那个终端里的 git 打架，
        // 顺带也更快
        .env("GIT_OPTIONAL_LOCKS", "0")
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                format!("capability_unavailable: 这台机器上没有 {program}")
            }
            _ => format!("{program} 起不来: {e}"),
        })?;

    // 看门狗：超时就杀掉。std 里没有带超时的 wait，所以自己起一个线程守着 ——
    // 引一个异步运行时只为了这一件事不值得（核心那 4 个依赖的约束是认真的）。
    //
    // **必须用 channel 通知，不能让看门狗自己轮询到点。** 第一版我写的是
    // 「睡满 TIMEOUT 再杀」，结果是 `join()` 要等满 10 秒 —— 每一次调用都变成 10 秒，
    // 而 git 本身 100 毫秒就回来了。`recv_timeout` 让正常路径立刻收摊。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn({
        let id = child.id();
        move || match rx.recv_timeout(TIMEOUT) {
            // 进程正常结束，主线程发了信号
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => false,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                kill(id);
                true
            }
        }
    });

    let done = child
        .wait_with_output()
        .map_err(|e| format!("{program} 收不到输出: {e}"))?;
    // 发不出去（看门狗已经超时退出）不是错误，下面 join 会拿到 true
    let _ = tx.send(());
    if watchdog.join().unwrap_or(false) {
        return Err(format!(
            "timeout: {program} 超过 {} 秒没结束",
            TIMEOUT.as_secs()
        ));
    }

    let (stdout, t1) = cap(String::from_utf8_lossy(&done.stdout).into_owned());
    let (stderr, t2) = cap(String::from_utf8_lossy(&done.stderr).into_owned());
    Ok(Output {
        exit: done.status.code().unwrap_or(-1),
        stdout,
        stderr,
        truncated: t1 || t2,
    })
}

/// 杀掉超时的进程。
///
/// 用系统自带的命令而不是引 `windows-sys`：这个 crate 要保持零依赖，
/// 而超时是异常路径，多花几十毫秒起一个 taskkill 完全可以接受。
#[cfg(windows)]
fn kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(windows))]
fn kill(pid: u32) {
    let _ = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_ids_this_crate_declares_are_reachable() {
        // 程序舱在清单里申报什么都越不出宿主这份白名单
        assert!(host_action("host.cli.rm", json!({})).is_err());
        assert!(host_action("host.cli.powershell", json!({})).is_err());
        assert!(host_action("host.cli.git", json!({}))
            .unwrap_err()
            .contains("invalid_input"));
        assert_eq!(ids(), &["host.cli.git"]);
    }

    #[test]
    fn an_unknown_op_is_refused_before_anything_runs() {
        let e = git(&json!({ "op": "push", "cwd": "C:/" })).unwrap_err();
        assert!(e.contains("不认得的 op"), "{e}");
        // 关键：拒的是 op 本身，而不是靠 git 去失败 —— 靠 git 失败等于
        // 「只要 git 认，程序舱就能跑」
        assert!(git(&json!({ "op": "config", "cwd": "C:/" })).is_err());
        assert!(git(&json!({ "op": "", "cwd": "C:/" })).is_err());
    }

    #[test]
    fn cwd_must_be_an_existing_absolute_directory() {
        assert!(checked_dir("").is_err());
        assert!(checked_dir("relative/path").is_err());
        assert!(checked_dir("C:/definitely/not/here/at/all").is_err());
        let tmp = std::env::temp_dir();
        assert!(checked_dir(tmp.to_str().unwrap()).is_ok());
        // 文件不是目录
        let f = tmp.join("podapp-cli-not-a-dir.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(checked_dir(f.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(f);
    }

    #[test]
    fn output_is_truncated_on_a_char_boundary() {
        // 切在 UTF-8 中间会让 JSON 序列化炸，而报错跟「输出太大」看不出关系
        let s = "中".repeat(MAX_OUTPUT);
        let (out, truncated) = cap(s);
        assert!(truncated);
        assert!(out.len() <= MAX_OUTPUT);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    /// 真跑一次 git。
    ///
    /// 机器上没有 git 就**明说跳过**，不静默通过 —— 一条什么都没验却绿着的测试
    /// 比红的更坏。
    #[test]
    fn git_status_really_runs_in_a_temp_repo() {
        if Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("[跳过] 这台机器上没有 git，git_status_really_runs 没有真正验证");
            return;
        }
        let dir = std::env::temp_dir().join(format!("podapp-cli-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
        ] {
            let _ = Command::new("git")
                .args(&args)
                .current_dir(&dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();

        let cwd = dir.to_str().unwrap();
        let r = git(&json!({ "op": "status", "cwd": cwd })).expect("status 该跑通");
        assert_eq!(r["exit"], 0, "{r}");
        // 新文件在 porcelain 里是 `?? a.txt`
        assert!(
            r["stdout"].as_str().unwrap().contains("a.txt"),
            "status 没看到新文件: {}",
            r["stdout"]
        );
        assert_eq!(r["truncated"], false);

        let l = git(&json!({ "op": "log", "cwd": cwd, "limit": 5 })).expect("log 该跑通");
        // 空仓库 log 会非零退出，但**不该是错误** —— 命令跑失败不是协议失败，
        // 结果里带上 exit 和 stderr 让调用方自己判
        assert!(l["exit"].is_number());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
