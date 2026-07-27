//! 无头执行 —— parity 的命门。
//!
//! GUI 那面点按钮走 `rpc`；而 CLI / MCP / 影核这三面没有 webview，靠的就是本模块：
//! 用宿主自带的 Node import **同一个** actions 模块。
//!
//! 一份实现两个面 —— 不是两边各写一遍然后祈祷它们一致。那样第一次改需求就分叉，
//! 而且分叉后 GUI 看着还是对的，AI 那条路悄悄坏掉，没人会发现。
//!
//! 权限闸装在 [`dispatch_capability`]，也就是**面之下**：GUI、无头、devtools 走同一道门。

use crate::action_spec::validate_input;
use crate::manifest::{load_dir, owner_of, resolve_dir};
use crate::perms::{permits, Cap};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 宿主能力回调。由调用方注入实现 —— 本 crate 永远拿不到 API Key（拿不到的东西泄不了）。
pub trait HostBridge: Send + Sync {
    fn ai_image_edit(&self, args: &Value) -> Result<Value, String>;
    fn ai_image_generate(&self, args: &Value) -> Result<Value, String>;
    fn ai_chat(&self, args: &Value) -> Result<Value, String>;
    fn file_save(&self, name: &str, data_url: &str) -> Result<Value, String>;
    fn file_open(&self, filters: &[String]) -> Result<Value, String>;
    fn host_action(&self, id: &str, input: Value) -> Result<Value, String>;
}

type HostActionFn = dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync;

/// 无头场景（CLI / MCP / 影核）用的宿主实现。
///
/// 需要用户点选的能力一律拒绝 —— 无人值守时弹「另存为」是错的，
/// 该由调用方拿着返回的数据自己落盘。
#[derive(Default)]
pub struct HeadlessHost {
    /// 宿主动作总线。不给就是无头下不放行宿主动作 —— 默认拒绝，而不是默认放行。
    host_actions: Option<Box<HostActionFn>>,
}

impl HeadlessHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// 接上宿主的动作总线，让程序舱能调它**已获准**的宿主动作。
    pub fn with_host_actions(
        f: impl Fn(&str, Value) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        Self { host_actions: Some(Box::new(f)) }
    }
}

impl HostBridge for HeadlessHost {
    fn ai_image_edit(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 无头模式的 AI 图像能力尚未接入".into())
    }
    fn ai_image_generate(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 无头模式的 AI 图像能力尚未接入".into())
    }
    fn ai_chat(&self, _a: &Value) -> Result<Value, String> {
        Err("capability_unavailable: 无头模式的 AI 对话能力尚未接入".into())
    }
    fn file_save(&self, _n: &str, _d: &str) -> Result<Value, String> {
        Err("capability_denied: 无头模式不弹文件对话框；请在结果里返回数据，由调用方落盘".into())
    }
    fn file_open(&self, _f: &[String]) -> Result<Value, String> {
        Err("capability_denied: 无头模式不弹文件对话框；请把内容放进入参".into())
    }
    fn host_action(&self, id: &str, input: Value) -> Result<Value, String> {
        match &self.host_actions {
            Some(f) => f(id, input),
            None => Err(format!("capability_unavailable: 这个宿主没有接动作总线，调不了 {id}")),
        }
    }
}

/// 处理一次能力请求。**权限闸在这里，且在 [`HostBridge`] 被调用之前。**
pub fn dispatch_capability(
    pod_id: &str,
    verb: &str,
    args: &Value,
    host: &dyn HostBridge,
) -> Result<Value, String> {
    let deny = |cap: &str| Err(format!("permission_denied: 这个程序舱没有申请 {cap} 权限"));
    match verb {
        "ai.image_edit" => {
            if !permits(pod_id, Cap::AiImageEdit) {
                return deny("ai.image_edit");
            }
            host.ai_image_edit(args)
        }
        "ai.image_generate" => {
            if !permits(pod_id, Cap::AiImageGenerate) {
                return deny("ai.image_generate");
            }
            host.ai_image_generate(args)
        }
        "ai.chat" => {
            if !permits(pod_id, Cap::AiChat) {
                return deny("ai.chat");
            }
            host.ai_chat(args)
        }
        "file.save" => {
            if !permits(pod_id, Cap::FsSaveDialog) {
                return deny("fs.save_dialog");
            }
            host.file_save(
                args.get("name").and_then(|v| v.as_str()).unwrap_or("output"),
                args.get("dataUrl").and_then(|v| v.as_str()).unwrap_or(""),
            )
        }
        "file.open" => {
            if !permits(pod_id, Cap::FsOpenDialog) {
                return deny("fs.open_dialog");
            }
            let f: Vec<String> = args
                .get("filters")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            host.file_open(&f)
        }
        "storage.get" | "storage.set" => {
            if !permits(pod_id, Cap::FsAppData) {
                return deny("fs.app_data");
            }
            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
            // key 会被拼成文件名 —— 放行 `/`、`\`、`.` 就等于放行路径穿越
            if key.is_empty() || key.len() > 128 || key.contains(['/', '\\', '.', '\0']) {
                return Err("invalid_input: 非法的 storage key".into());
            }
            let dir = crate::data_dir(pod_id).join("kv");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let f = dir.join(format!("{key}.json"));
            if verb == "storage.get" {
                Ok(std::fs::read_to_string(&f)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(Value::Null))
            } else {
                let v = args.get("value").cloned().unwrap_or(Value::Null);
                std::fs::write(&f, v.to_string()).map_err(|e| e.to_string())?;
                Ok(json!({ "ok": true }))
            }
        }
        // 产出箱：程序舱把成品交给宿主，由宿主决定「给谁看、在哪看」。
        // 不需要单独权限：交产物是把东西**给**用户，不是拿用户的东西。
        "artifact.emit" => {
            let data = args.get("data").and_then(|v| v.as_str()).unwrap_or("");
            if data.is_empty() {
                return Err("invalid_input: artifact.emit 缺少 data".into());
            }
            let a = crate::artifacts::emit(
                pod_id,
                args.get("action").and_then(|v| v.as_str()),
                args.get("kind").and_then(|v| v.as_str()).unwrap_or("image"),
                data,
                args.get("message").and_then(|v| v.as_str()),
            )?;
            let path = crate::artifacts::path_of(&a.id).map(|p| p.display().to_string());
            Ok(json!({
                "id": a.id, "kind": a.kind, "w": a.w, "h": a.h, "bytes": a.bytes, "path": path
            }))
        }
        "action" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let input = args.get("input").cloned().unwrap_or(json!({}));
            if id.starts_with("app.") {
                // 程序舱互调还没设计。明确拒绝好过含糊放行 —— 含糊放行会让第一个
                // 用上它的人依赖一套没想清楚的语义，之后想改就是破坏性变更。
                return Err("not_implemented: 程序舱之间互相调用尚未支持".into());
            }
            let allowed = crate::manifest::permissions(pod_id)
                .map(|p| p.host_actions.iter().any(|h| h == id))
                .unwrap_or(false);
            if !allowed {
                return Err(format!("permission_denied: 这个程序舱没有申请调用宿主动作 {id}"));
            }
            host.host_action(id, input)
        }
        // 图像原语不需要权限：它们只在内存里搬像素，碰不到网络也碰不到用户的盘。
        // 真正要闸的是「图从哪来」（file.open）和「图往哪去」（file.save / artifact.emit）。
        v if v.starts_with("image.") => crate::image::dispatch(&v[6..], args),
        other => Err(format!("unknown_capability: {other}")),
    }
}

/// GUI 侧的一次能力请求（来自 `<scheme>://localhost/rpc/<pod-id>/<verb>`）。
/// 与无头路径共用同一个 [`dispatch_capability`] —— 权限闸在面之下，全局只有一道。
pub fn rpc(pod_id: &str, verb: &str, args: &Value, host: &dyn HostBridge) -> Result<Value, String> {
    if resolve_dir(pod_id).is_none() {
        return Err(format!("unknown_pod: {pod_id}"));
    }
    // 程序舱在自己页面里调自己的动作，走的仍是同一条动作总线
    if verb == "action" {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let input = args.get("input").cloned().unwrap_or(json!({}));
        if let Some(owner) = owner_of(id) {
            if owner != pod_id {
                return Err("permission_denied: 不能调用别的程序舱的动作".into());
            }
            return run_action_with(id, input, host);
        }
    }
    dispatch_capability(pod_id, verb, args, host)
}

/// 找 node：`<PREFIX>_NODE` → 宿主便携 Node → 系统 PATH → 常见安装位置。
fn find_node() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(format!("{}_NODE", crate::profile().env_prefix)) {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let names: &[&str] = if cfg!(windows) { &["node.exe", "node"] } else { &["node"] };
    let portable = crate::home().join("runtime");
    for sub in ["node-win-x64", "node", "node-x64"] {
        for n in names {
            for c in [portable.join(sub).join(n), portable.join(sub).join("bin").join(n)] {
                if c.exists() {
                    return Some(c);
                }
            }
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path.split(sep).filter(|d| !d.is_empty()) {
            for n in names {
                let c = Path::new(dir).join(n);
                if c.exists() {
                    return Some(c);
                }
            }
        }
    }
    #[cfg(windows)]
    for base in ["C:\\Program Files\\nodejs\\node.exe", "C:\\Program Files (x86)\\nodejs\\node.exe"]
    {
        let p = Path::new(base);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// 无头跑一个动作，不接宿主能力。
pub fn run_action(action_id: &str, input: Value) -> Result<Value, String> {
    run_action_with(action_id, input, &HeadlessHost::new())
}

/// 执行一个程序舱动作。GUI 侧传带真实能力的 `host`；无头侧传 [`HeadlessHost`]。
pub fn run_action_with(
    action_id: &str,
    input: Value,
    host: &dyn HostBridge,
) -> Result<Value, String> {
    use std::io::{BufRead, BufReader, Write};

    let pod_id = owner_of(action_id).ok_or_else(|| format!("unknown_action: {action_id}"))?;
    let dir = resolve_dir(&pod_id).ok_or_else(|| format!("unknown_action: {action_id}"))?;
    let (m, parity) = load_dir(&dir)?;

    let spec = parity
        .get("actions")
        .and_then(|v| v.as_array())
        .and_then(|a| a.iter().find(|x| x.get("id").and_then(|v| v.as_str()) == Some(action_id)))
        .ok_or_else(|| format!("unknown_action: {action_id}"))?
        .clone();

    // 副作用之前先校验入参 —— agent 会瞎试，声明的 schema 是唯一护栏
    if let Some(s) = spec.get("input_schema") {
        validate_input(s, &input, "input")?;
    }
    if spec.pointer("/execution/headless").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!(
            "not_headless: 动作 {action_id} 声明了 headless=false，只能在界面里用"
        ));
    }
    if m.package.kind != "web" {
        return Err(format!("not_implemented: {} 形态的无头执行尚未接入", m.package.kind));
    }

    let w = m.package.web.clone().unwrap_or_default();
    let am = w.actions.clone().ok_or("这个程序舱没有动作模块")?;
    let root = crate::safe_join(&dir, &w.root).ok_or("package.web.root 非法")?;
    let module = crate::safe_join(&root, &am).ok_or("package.web.actions 非法")?;
    let node = find_node()
        .ok_or("找不到 Node —— 程序舱的无头执行需要它。装一个系统 Node，或设 PODAPP_NODE 指过去。")?;

    let tmp = std::env::temp_dir().join(format!("podapp-run-{}", crate::now_ms()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let runner = tmp.join("runner.mjs");
    let inf = tmp.join("input.json");
    let outf = tmp.join("output.json");
    std::fs::write(&runner, crate::bridge::RUNNER_JS).map_err(|e| e.to_string())?;
    std::fs::write(&inf, input.to_string()).map_err(|e| e.to_string())?;

    // 沙箱：动作模块跑在**宿主自己的 Node 进程**里，默认能读整个用户目录。
    //
    // 实测过一个十几行的恶意 actions.mjs：裸跑读出了家目录全部内容（凭据就在里面），
    // 还能起子进程。也就是说「绝不下发凭据」光靠桥上没有 fetch 是假的 ——
    // 它能绕过桥、直接从磁盘把 Key 读走。
    //
    // 规范里写「模块 MUST NOT 碰 fs」是对作者的**要求**；这里是让它**做不到**。
    // 两者差别是生死攸关的，不能只写在纸上。
    // 只开两个目录：程序舱自己的目录（读）+ 本次运行的临时目录（读写）。
    let allow_read_app = format!("--allow-fs-read={}", root.display());
    let allow_read_tmp = format!("--allow-fs-read={}", tmp.display());
    let allow_write_tmp = format!("--allow-fs-write={}", tmp.display());
    let mut child = std::process::Command::new(&node)
        .arg("--experimental-permission")
        .arg(&allow_read_app)
        .arg(&allow_read_tmp)
        .arg(&allow_write_tmp)
        .arg(&runner)
        .arg(&module)
        .arg(action_id)
        .arg(&inf)
        .arg(&outf)
        .current_dir(&tmp)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("起不来 Node: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("拿不到子进程 stdin")?;
    let stdout = child.stdout.take().ok_or("拿不到子进程 stdout")?;
    let mut ai_calls = 0u32;
    let max_calls = m.permissions.ai.max_calls_per_run;

    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Some(payload) = line.strip_prefix('\u{1}') else {
            // 程序舱自己 console.log 的东西，不当协议看
            eprintln!("[pod:{pod_id}] {line}");
            continue;
        };
        let req: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        let verb = req.get("verb").and_then(|v| v.as_str()).unwrap_or("");
        let args = req.get("args").cloned().unwrap_or(json!({}));

        // 额度硬闸：跑飞的循环不许烧光用户的钱
        let over = verb.starts_with("ai.") && {
            ai_calls += 1;
            ai_calls > max_calls
        };
        let resp = if over {
            json!({ "ok": false, "error": format!("quota_exceeded: 本轮最多调用 {max_calls} 次 AI") })
        } else {
            match dispatch_capability(&pod_id, verb, &args, host) {
                Ok(d) => json!({ "ok": true, "data": d }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        };
        if writeln!(stdin, "{resp}").is_err() {
            break;
        }
        let _ = stdin.flush();
    }
    drop(stdin);

    let status = child.wait().map_err(|e| e.to_string())?;
    crate::image::clear_session();
    let out = std::fs::read_to_string(&outf).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);

    let v: Value = serde_json::from_str(&out)
        .map_err(|_| format!("动作没有产出结果（Node 退出码 {:?}）", status.code()))?;
    if v.get("ok").and_then(|x| x.as_bool()) == Some(true) {
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    } else {
        Err(v.get("error").and_then(|x| x.as_str()).unwrap_or("动作执行失败").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_refuses_interactive_capabilities() {
        let h = HeadlessHost::new();
        // 无人值守时弹「另存为」是错的，必须明确拒绝而不是挂在那里等
        assert!(h.file_save("a", "data:,x").is_err());
        assert!(h.file_open(&[]).is_err());
        assert!(h.host_action("host.zip.pack", json!({})).is_err(), "没接总线就不该放行");
    }

    #[test]
    fn host_actions_reach_the_injected_bus() {
        let h = HeadlessHost::with_host_actions(|id, _| Ok(json!({ "called": id })));
        assert_eq!(h.host_action("host.zip.pack", json!({})).unwrap()["called"], "host.zip.pack");
    }

    #[test]
    fn unknown_capability_is_rejected_not_ignored() {
        // 没装的 pod 也走同一条路：未知能力必须报错，静默返回 null 会让作者以为成功了
        let e = dispatch_capability("nope", "totally.made.up", &json!({}), &HeadlessHost::new())
            .unwrap_err();
        assert!(e.starts_with("unknown_capability"), "实际: {e}");
    }

    #[test]
    fn storage_key_cannot_traverse() {
        let h = HeadlessHost::new();
        for bad in ["../escape", "a/b", "a\\b", "with.dot", ""] {
            let e = dispatch_capability("x", "storage.set", &json!({ "key": bad }), &h).unwrap_err();
            // 要么被 key 校验拦下，要么因为 pod 没装而被权限闸拦下 —— 都不能落盘
            assert!(
                e.contains("invalid_input") || e.contains("permission_denied"),
                "key={bad:?} 实际: {e}"
            );
        }
    }
}
