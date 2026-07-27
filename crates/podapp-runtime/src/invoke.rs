//! 一次动作调用的信封 —— 影核（ActionParity）§10.3 / §10.4 要的那几样。
//!
//! 只传 `(action_id, input)` 对本机点按钮够用，对**影子**（手机、远端、另一台设备）不够。
//! 规范和开发宪法第 16 条要求远程写带幂等键和 `expected_state_version`，
//! 且「最后写入者获胜」不许当未声明的默认。
//!
//! 这三样现在就加，不是因为 M1 用得上，而是因为**信封形状是外部契约**：
//! 等影子那条路真接上再改，所有调用点、所有已发布的程序舱绑定都要跟着动。
//!
//! ## 运行时管协议，宿主管语义
//!
//! 「当前状态版本是多少」只有宿主知道（它才知道那个动作写的是哪份状态）。
//! 所以运行时**强制流程**：带了 `expected_state_version` 就必须问过 [`StateResolver`]，
//! 对不上就返回 `conflict` 且**不执行**。宿主不提供 resolver 时，带版本的调用一律拒绝 ——
//! 假装检查过是最坏的一种：调用方以为自己受保护，其实在裸奔。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// 一次调用的完整信封。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invocation {
    pub action_id: String,
    pub input: Value,
    /// 贯穿请求 → 结果 → 事件 → 日志的关联 ID（§10.4）。构造时自动生成。
    pub execution_id: String,
    /// 幂等键。同一个键重放直接返回上次结果，**不重复执行**。
    /// 影子那条路上重试是常态（网断了不知道对面执行没执行），没有它就只能祈祷动作本身幂等。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// 调用方观察到的状态版本。与当前不符则冲突，拒绝执行（§10.3 optimistic）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_state_version: Option<String>,
}

impl Invocation {
    pub fn new(action_id: impl Into<String>, input: Value) -> Self {
        Self {
            action_id: action_id.into(),
            input,
            execution_id: new_execution_id(),
            idempotency_key: None,
            expected_state_version: None,
        }
    }

    pub fn with_idempotency_key(mut self, k: impl Into<String>) -> Self {
        self.idempotency_key = Some(k.into());
        self
    }

    pub fn with_expected_state_version(mut self, v: impl Into<String>) -> Self {
        self.expected_state_version = Some(v.into());
        self
    }
}

/// 宿主告诉运行时「这个动作写的那份状态，现在是什么版本」。
///
/// 返回 `None` = 这个动作不写共享状态，版本检查跳过。
pub trait StateResolver: Send + Sync {
    fn version_of(&self, action_id: &str) -> Option<String>;
}

/// 进程内唯一的执行 ID。不引随机数依赖：时间 + 进程 + 自增计数三者合起来
/// 在单机上足够唯一，而跨机唯一性本来就该由影子那边的会话 ID 负责。
fn new_execution_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("exec_{:x}_{:x}_{n:x}", crate::now_ms(), std::process::id())
}

/// 幂等重放缓存：插入顺序 + 键到结果的映射。顺序那一半用来淘汰最旧的。
type ReplayCache = (Vec<String>, HashMap<String, Value>);

/// 有界，超了丢最旧 —— 无界缓存在长跑的宿主里就是内存泄漏。
const REPLAY_MAX: usize = 256;
static REPLAY: Mutex<Option<ReplayCache>> = Mutex::new(None);

fn replay_key(pod_id: &str, inv: &Invocation) -> Option<String> {
    inv.idempotency_key
        .as_ref()
        .map(|k| format!("{pod_id}\u{1}{}\u{1}{k}", inv.action_id))
}

pub(crate) fn replay_lookup(pod_id: &str, inv: &Invocation) -> Option<Value> {
    let k = replay_key(pod_id, inv)?;
    REPLAY.lock().ok()?.as_ref()?.1.get(&k).cloned()
}

pub(crate) fn replay_store(pod_id: &str, inv: &Invocation, out: &Value) {
    let Some(k) = replay_key(pod_id, inv) else {
        return;
    };
    let Ok(mut g) = REPLAY.lock() else { return };
    let (order, map) = g.get_or_insert_with(|| (Vec::new(), HashMap::new()));
    if map.insert(k.clone(), out.clone()).is_none() {
        order.push(k);
        while order.len() > REPLAY_MAX {
            let old = order.remove(0);
            map.remove(&old);
        }
    }
}

/// 副作用之前的守门：版本对不对、要不要直接返回上次结果。
///
/// `Ok(Some(v))` = 命中幂等重放，直接把 v 还给调用方，别执行。
/// `Ok(None)` = 放行。
/// `Err` = 冲突或缺 resolver，**不执行**。
pub(crate) fn guard(
    pod_id: &str,
    inv: &Invocation,
    state: Option<&dyn StateResolver>,
) -> Result<Option<Value>, String> {
    if let Some(expected) = &inv.expected_state_version {
        let Some(state) = state else {
            return Err(format!(
                "precondition_unsupported: 调用带了 expected_state_version，\
                 但这个宿主没接 StateResolver —— 拒绝执行好过假装检查过（execution_id={})",
                inv.execution_id
            ));
        };
        if let Some(current) = state.version_of(&inv.action_id) {
            if &current != expected {
                return Err(format!(
                    "conflict: 状态在你读到之后被改过（你看到 {expected}，现在是 {current}）——\
                     重新读一次再试（execution_id={}）",
                    inv.execution_id
                ));
            }
        }
    }
    Ok(replay_lookup(pod_id, inv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixed(&'static str);
    impl StateResolver for Fixed {
        fn version_of(&self, _a: &str) -> Option<String> {
            Some(self.0.into())
        }
    }
    struct Stateless;
    impl StateResolver for Stateless {
        fn version_of(&self, _a: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn execution_ids_are_unique() {
        let a: Vec<String> = (0..100).map(|_| new_execution_id()).collect();
        let uniq: std::collections::HashSet<_> = a.iter().collect();
        assert_eq!(uniq.len(), a.len(), "关联 ID 撞了，日志就串了");
    }

    #[test]
    fn a_plain_call_passes_the_guard() {
        let inv = Invocation::new("app.x.y.z", json!({}));
        assert!(inv.expected_state_version.is_none());
        assert_eq!(guard("p", &inv, None).unwrap(), None);
    }

    #[test]
    fn stale_version_is_a_conflict_and_does_not_execute() {
        let inv = Invocation::new("app.x.y.z", json!({})).with_expected_state_version("v1");
        let e = guard("p", &inv, Some(&Fixed("v2"))).unwrap_err();
        assert!(e.starts_with("conflict:"), "实际: {e}");
        assert!(
            e.contains("execution_id="),
            "冲突信息该带关联 ID，否则查不到是哪一次"
        );
    }

    #[test]
    fn matching_version_passes() {
        let inv = Invocation::new("app.x.y.z", json!({})).with_expected_state_version("v1");
        assert_eq!(guard("p", &inv, Some(&Fixed("v1"))).unwrap(), None);
    }

    #[test]
    fn an_action_that_writes_no_shared_state_skips_the_check() {
        let inv = Invocation::new("app.x.y.z", json!({})).with_expected_state_version("whatever");
        assert_eq!(guard("p", &inv, Some(&Stateless)).unwrap(), None);
    }

    #[test]
    fn version_check_without_a_resolver_is_refused_not_skipped() {
        // 最坏的实现是「没 resolver 就跳过检查」：调用方以为自己受保护，其实在裸奔
        let inv = Invocation::new("app.x.y.z", json!({})).with_expected_state_version("v1");
        let e = guard("p", &inv, None).unwrap_err();
        assert!(e.starts_with("precondition_unsupported:"), "实际: {e}");
    }

    #[test]
    fn the_same_idempotency_key_replays_instead_of_re_executing() {
        let inv = Invocation::new("app.x.replay.run", json!({ "n": 1 })).with_idempotency_key("k1");
        assert_eq!(guard("pod-a", &inv, None).unwrap(), None, "第一次该放行");
        replay_store("pod-a", &inv, &json!({ "done": true }));

        // 重试（比如影子那边网断了重发）：返回上次结果，不再执行
        let retry =
            Invocation::new("app.x.replay.run", json!({ "n": 1 })).with_idempotency_key("k1");
        assert_eq!(
            guard("pod-a", &retry, None).unwrap(),
            Some(json!({ "done": true }))
        );

        // 换个键 / 换个程序舱 / 换个动作，都是另一次调用
        let other_key = Invocation::new("app.x.replay.run", json!({})).with_idempotency_key("k2");
        assert_eq!(guard("pod-a", &other_key, None).unwrap(), None);
        assert_eq!(
            guard("pod-b", &retry, None).unwrap(),
            None,
            "别的程序舱不该命中"
        );
    }

    #[test]
    fn no_key_means_no_replay() {
        let inv = Invocation::new("app.x.nokey.run", json!({}));
        replay_store("pod-a", &inv, &json!({ "done": true }));
        assert_eq!(
            guard("pod-a", &inv, None).unwrap(),
            None,
            "没给键就不该被当成重放"
        );
    }
}
