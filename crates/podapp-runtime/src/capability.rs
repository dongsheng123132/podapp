//! 能力注册表 —— 程序舱能从桥上调到的那些动词，谁提供、要什么权限。
//!
//! ## 为什么不是一个 `match`
//!
//! 原来这里是一大段 `match verb { ... }`，每个分支自己先查一遍权限再干活。两个问题：
//!
//! 1. **加能力必须改核心。** 第三方想加 `pdf.*` 或 `qr.scan` 只能 fork 整个运行时。
//!    「开放的底层壳子」和「能力写死在壳子里」是矛盾的。
//! 2. **闸门被抄了七遍。** 每个分支各写一次 `if !permits(...) { deny }`，
//!    只要有一个分支忘了写，那条路就是敞开的 —— 而这种缺失不会有任何症状，
//!    直到有人发现某个动词不用申请权限也能调。
//!
//! 换成注册表之后：能力**声明**自己要什么权限，闸门由 [`Capabilities::dispatch`]
//! **统一执行一次**。想漏都漏不掉，因为提供方根本没有放行的权力。
//!
//! ## 闸门仍然在面之下
//!
//! GUI、无头、devtools 三条路都汇到 [`Capabilities::dispatch`]。这一点没变，
//! 也不能变 —— 规范里写「程序舱 MUST NOT 越权」是对作者的要求，这里是让它做不到。

use crate::perms::Cap;
use crate::HostBridge;
use serde_json::{json, Value};

/// 一次能力调用的上下文。
pub struct CapCtx<'a> {
    pub pod_id: &'a str,
    /// 宿主注入的真实能力（AI / 文件对话框 / 宿主动作总线）
    pub host: &'a dyn HostBridge,
    /// 贯穿请求 → 结果 → 事件 → 日志的关联 ID（ActionParity §10.4）
    pub execution_id: &'a str,
}

/// 一组动词的提供方。
///
/// 实现它就能给运行时加能力，不用改运行时本身。
pub trait Capability: Send + Sync {
    /// 诊断用的名字，出现在错误信息和日志里。
    fn name(&self) -> &'static str;

    /// 这个动词归不归我管。
    fn handles(&self, verb: &str) -> bool;

    /// 这个动词要哪项权限。`None` = 不需要申请。
    ///
    /// **只是声明，不是执行。** 执行在 [`Capabilities::dispatch`] 里，提供方无权放行。
    fn required(&self, verb: &str) -> Option<Cap>;

    fn call(&self, ctx: &CapCtx, verb: &str, args: &Value) -> Result<Value, String>;
}

/// 已装配的能力集合。宿主启动时组装，之后只读。
pub struct Capabilities {
    items: Vec<Box<dyn Capability>>,
}

impl Capabilities {
    /// 只有内置能力。
    pub fn builtin() -> Self {
        Self {
            items: vec![
                Box::new(AiCap),
                Box::new(FileCap),
                Box::new(StorageCap),
                Box::new(ArtifactCap),
                Box::new(ImageCap),
                Box::new(HostActionCap),
            ],
        }
    }

    /// 空集合 —— 给只想要极小面的宿主用（比如纯校验器、CI 里的 lint）。
    pub fn none() -> Self {
        Self { items: vec![] }
    }

    /// 加一个能力。**后加的先匹配**，所以宿主可以覆盖内置动词。
    pub fn with(mut self, c: impl Capability + 'static) -> Self {
        self.items.insert(0, Box::new(c));
        self
    }

    /// 当前认识的能力名字，用于诊断（「我到底装了些什么」）。
    pub fn names(&self) -> Vec<&'static str> {
        self.items.iter().map(|c| c.name()).collect()
    }

    /// 分发一次调用。**唯一的权限闸在这里。**
    pub fn dispatch(&self, ctx: &CapCtx, verb: &str, args: &Value) -> Result<Value, String> {
        let Some(c) = self.items.iter().find(|c| c.handles(verb)) else {
            return Err(format!("unknown_capability: {verb}"));
        };
        if let Some(cap) = c.required(verb) {
            if !crate::perms::permits(ctx.pod_id, cap) {
                return Err(format!("permission_denied: 这个程序舱没有申请 {verb} 需要的权限"));
            }
        }
        c.call(ctx, verb, args)
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::builtin()
    }
}

// ───────────────────────────── 内置能力 ─────────────────────────────

struct AiCap;
impl Capability for AiCap {
    fn name(&self) -> &'static str {
        "ai"
    }
    fn handles(&self, v: &str) -> bool {
        matches!(v, "ai.image_edit" | "ai.image_generate" | "ai.chat")
    }
    fn required(&self, v: &str) -> Option<Cap> {
        match v {
            "ai.image_edit" => Some(Cap::AiImageEdit),
            "ai.image_generate" => Some(Cap::AiImageGenerate),
            _ => Some(Cap::AiChat),
        }
    }
    fn call(&self, ctx: &CapCtx, v: &str, a: &Value) -> Result<Value, String> {
        match v {
            "ai.image_edit" => ctx.host.ai_image_edit(a),
            "ai.image_generate" => ctx.host.ai_image_generate(a),
            _ => ctx.host.ai_chat(a),
        }
    }
}

struct FileCap;
impl Capability for FileCap {
    fn name(&self) -> &'static str {
        "file"
    }
    fn handles(&self, v: &str) -> bool {
        matches!(v, "file.save" | "file.open")
    }
    fn required(&self, v: &str) -> Option<Cap> {
        Some(if v == "file.save" { Cap::FsSaveDialog } else { Cap::FsOpenDialog })
    }
    fn call(&self, ctx: &CapCtx, v: &str, a: &Value) -> Result<Value, String> {
        if v == "file.save" {
            ctx.host.file_save(
                a.get("name").and_then(|x| x.as_str()).unwrap_or("output"),
                a.get("dataUrl").and_then(|x| x.as_str()).unwrap_or(""),
            )
        } else {
            let f: Vec<String> = a
                .get("filters")
                .and_then(|x| x.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            ctx.host.file_open(&f)
        }
    }
}

struct StorageCap;
impl Capability for StorageCap {
    fn name(&self) -> &'static str {
        "storage"
    }
    fn handles(&self, v: &str) -> bool {
        matches!(v, "storage.get" | "storage.set")
    }
    fn required(&self, _v: &str) -> Option<Cap> {
        Some(Cap::FsAppData)
    }
    fn call(&self, ctx: &CapCtx, v: &str, a: &Value) -> Result<Value, String> {
        let key = a.get("key").and_then(|x| x.as_str()).unwrap_or("");
        // key 会被拼成文件名 —— 放行 `/`、`\`、`.` 就等于放行路径穿越
        if key.is_empty() || key.len() > 128 || key.contains(['/', '\\', '.', '\0']) {
            return Err("invalid_input: 非法的 storage key".into());
        }
        let dir = crate::data_dir(ctx.pod_id).join("kv");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let f = dir.join(format!("{key}.json"));
        if v == "storage.get" {
            Ok(std::fs::read_to_string(&f)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null))
        } else {
            let val = a.get("value").cloned().unwrap_or(Value::Null);
            std::fs::write(&f, val.to_string()).map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true }))
        }
    }
}

struct ArtifactCap;
impl Capability for ArtifactCap {
    fn name(&self) -> &'static str {
        "artifact"
    }
    fn handles(&self, v: &str) -> bool {
        v == "artifact.emit"
    }
    /// 不需要权限：交产物是把东西**给**用户，不是拿用户的东西。
    fn required(&self, _v: &str) -> Option<Cap> {
        None
    }
    fn call(&self, ctx: &CapCtx, _v: &str, a: &Value) -> Result<Value, String> {
        let data = a.get("data").and_then(|x| x.as_str()).unwrap_or("");
        if data.is_empty() {
            return Err("invalid_input: artifact.emit 缺少 data".into());
        }
        let art = crate::artifacts::emit(
            ctx.pod_id,
            a.get("action").and_then(|x| x.as_str()),
            a.get("kind").and_then(|x| x.as_str()).unwrap_or("image"),
            data,
            a.get("message").and_then(|x| x.as_str()),
        )?;
        let path = crate::artifacts::path_of(&art.id).map(|p| p.display().to_string());
        Ok(json!({
            "id": art.id, "kind": art.kind, "w": art.w, "h": art.h,
            "bytes": art.bytes, "path": path
        }))
    }
}

struct ImageCap;
impl Capability for ImageCap {
    fn name(&self) -> &'static str {
        "image"
    }
    fn handles(&self, v: &str) -> bool {
        v.starts_with("image.")
    }
    /// 图像原语不需要权限：它们只在内存里搬像素，碰不到网络也碰不到用户的盘。
    /// 真正要闸的是「图从哪来」（file.open）和「图往哪去」（file.save / artifact.emit）。
    fn required(&self, _v: &str) -> Option<Cap> {
        None
    }
    fn call(&self, _ctx: &CapCtx, v: &str, a: &Value) -> Result<Value, String> {
        crate::image::dispatch(&v["image.".len()..], a)
    }
}

struct HostActionCap;
impl Capability for HostActionCap {
    fn name(&self) -> &'static str {
        "action"
    }
    fn handles(&self, v: &str) -> bool {
        v == "action"
    }
    /// 逐个动作 ID 白名单，粒度比布尔权限细，所以在 `call` 里自己判。
    fn required(&self, _v: &str) -> Option<Cap> {
        None
    }
    fn call(&self, ctx: &CapCtx, _v: &str, a: &Value) -> Result<Value, String> {
        let id = a.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let input = a.get("input").cloned().unwrap_or(json!({}));
        if id.starts_with("app.") {
            // 程序舱互调还没设计。明确拒绝好过含糊放行 —— 含糊放行会让第一个用上它的人
            // 依赖一套没想清楚的语义，之后想改就是破坏性变更。
            return Err("not_implemented: 程序舱之间互相调用尚未支持".into());
        }
        let allowed = crate::manifest::permissions(ctx.pod_id)
            .map(|p| p.host_actions.iter().any(|h| h == id))
            .unwrap_or(false);
        if !allowed {
            return Err(format!("permission_denied: 这个程序舱没有申请调用宿主动作 {id}"));
        }
        ctx.host.host_action(id, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HeadlessHost;

    struct Probe;
    impl Capability for Probe {
        fn name(&self) -> &'static str {
            "probe"
        }
        fn handles(&self, v: &str) -> bool {
            v.starts_with("probe.")
        }
        fn required(&self, _v: &str) -> Option<Cap> {
            // 故意要一个未装程序舱必定没有的权限，用来验闸门确实执行了
            Some(Cap::AiChat)
        }
        fn call(&self, _c: &CapCtx, _v: &str, _a: &Value) -> Result<Value, String> {
            Ok(json!({ "reached": true }))
        }
    }

    struct Open;
    impl Capability for Open {
        fn name(&self) -> &'static str {
            "open"
        }
        fn handles(&self, v: &str) -> bool {
            v == "open.ping"
        }
        fn required(&self, _v: &str) -> Option<Cap> {
            None
        }
        fn call(&self, c: &CapCtx, _v: &str, _a: &Value) -> Result<Value, String> {
            Ok(json!({ "exec": c.execution_id }))
        }
    }

    fn ctx<'a>(host: &'a HeadlessHost, id: &'a str) -> CapCtx<'a> {
        CapCtx { pod_id: id, host, execution_id: "exec-test" }
    }

    #[test]
    fn a_third_party_capability_needs_no_core_change() {
        // 这条测试就是「可插拔」的定义：加能力不改运行时
        let caps = Capabilities::builtin().with(Open);
        let h = HeadlessHost::new();
        let out = caps.dispatch(&ctx(&h, "any"), "open.ping", &json!({})).unwrap();
        assert_eq!(out["exec"], "exec-test", "关联 ID 该传到能力里");
        assert!(caps.names().contains(&"open"));
    }

    #[test]
    fn the_gate_runs_even_for_capabilities_that_never_check() {
        // Probe 自己完全不查权限 —— 它声明了要 AiChat，闸门必须替它拦住。
        // 这正是从 match 换成注册表要买的东西：提供方没有放行的权力。
        let caps = Capabilities::builtin().with(Probe);
        let h = HeadlessHost::new();
        let e = caps.dispatch(&ctx(&h, "not-installed"), "probe.anything", &json!({})).unwrap_err();
        assert!(e.starts_with("permission_denied"), "实际: {e}");
    }

    #[test]
    fn later_registrations_win() {
        struct Hijack;
        impl Capability for Hijack {
            fn name(&self) -> &'static str {
                "hijack"
            }
            fn handles(&self, v: &str) -> bool {
                v.starts_with("image.")
            }
            fn required(&self, _v: &str) -> Option<Cap> {
                None
            }
            fn call(&self, _c: &CapCtx, _v: &str, _a: &Value) -> Result<Value, String> {
                Ok(json!("hijacked"))
            }
        }
        let caps = Capabilities::builtin().with(Hijack);
        let h = HeadlessHost::new();
        assert_eq!(caps.dispatch(&ctx(&h, "x"), "image.decode", &json!([])).unwrap(), "hijacked");
    }

    #[test]
    fn unknown_verbs_are_rejected_not_ignored() {
        let caps = Capabilities::builtin();
        let h = HeadlessHost::new();
        let e = caps.dispatch(&ctx(&h, "x"), "totally.made.up", &json!({})).unwrap_err();
        assert!(e.starts_with("unknown_capability"), "实际: {e}");
    }

    #[test]
    fn an_empty_registry_grants_nothing() {
        // 极小面宿主：连内置能力都不给。此时任何动词都该被拒，而不是意外放行。
        let caps = Capabilities::none();
        let h = HeadlessHost::new();
        for v in ["ai.chat", "file.save", "storage.get", "artifact.emit", "image.decode"] {
            assert!(caps.dispatch(&ctx(&h, "x"), v, &json!({})).is_err(), "{v} 该被拒");
        }
    }

    #[test]
    fn builtin_verbs_all_declare_a_gate_or_a_reason() {
        // 每个内置动词要么声明权限，要么属于「明确不需要权限」的白名单。
        // 新增内置能力时忘了想权限，这条会把它逼出来。
        let caps = Capabilities::builtin();
        let no_perm_needed = ["artifact.emit", "image.", "action"];
        for c in &caps.items {
            for v in [
                "ai.chat", "ai.image_edit", "ai.image_generate", "file.save", "file.open",
                "storage.get", "storage.set", "artifact.emit", "image.decode", "action",
            ] {
                if c.handles(v) && c.required(v).is_none() {
                    assert!(
                        no_perm_needed.iter().any(|p| v.starts_with(p)),
                        "{v} 由 {} 提供却不要任何权限，且不在豁免名单里",
                        c.name()
                    );
                }
            }
        }
    }
}
