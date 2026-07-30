//! 把几个动作串成一条工作流。
//!
//! # 为什么工作流是对的那块积木
//!
//! 用户知道自己的小需求：「身份证抠出来，加个白底，导出」。他不知道怎么写清单，
//! 更不会去建工程、构建 exe。
//!
//! 让 AI 替他写一个程序舱？那要写代码、写 `action-parity.json`、打包、验证 ——
//! 为一个三步的需求付一整个程序舱的代价。
//!
//! **工作流只是一张步骤表。** 它不含代码，只引用**已经装好的动作**：
//!
//! ```json
//! {
//!   "spec": "podapp/flow@0.1",
//!   "id": "my.idcard",
//!   "name": "身份证三步",
//!   "steps": [
//!     { "action": "app.annotate.image.crop", "input": { "image": "$in" } },
//!     { "action": "app.qrfix.code.replace",  "input": { "poster": "$prev" } }
//!   ]
//! }
//! ```
//!
//! AI 生成这个的成本比生成一个程序舱低一个数量级，而且**错了也不会装坏什么** ——
//! 它引用不到的动作在跑之前就会被拦下。
//!
//! # 三条设计
//!
//! **1. 跑之前先验完。** 引用了没装的动作、引用了跑不了无头的动作、`$prev` 出现在
//! 第一步 —— 这些在 [`check`] 里一次说清，不是跑到第二步才崩。
//! 小白拿到「第 2 步那个动作你没装」比拿到「运行失败」有用得多。
//!
//! **2. 不新增执行路径。** 每一步都落到
//! [`podapp_runtime::headless::invoke`] —— 跟人点按钮、跟 AI 走 MCP 是同一条。
//! 工作流只是**按顺序调**，它不是第二个执行引擎。
//!
//! **3. 要确认的步骤停下来。** 动作自己声明 `confirmation`（never / destructive /
//! always）。声明了要确认的，[`run`] 到那一步就停，把「下一步要干什么」交回调用方。
//! **绝不代替用户点头** —— 这正是浮舱存在的理由，不能在工作流这条路上绕开。
//!
//! # 为什么不进 `podapp-runtime`
//!
//! 跟 qr / zip / cli 一样：可插拔。删掉这个 crate，动作总线一点不变。
//! 而且它一个第三方依赖都不引 —— 用的全是运行时已经有的东西。

use podapp_runtime::{Capabilities, HostBridge, Invocation};
use serde_json::{json, Map, Value};

/// 上一步的产物。
pub const PREV: &str = "$prev";
/// 调用方喂进来的输入（用户拖进浮舱的那个东西）。
pub const IN: &str = "$in";

const SPEC: &str = "podapp/flow@0.1";
/// 步数上限。
///
/// 不是性能考虑，是**别让一条手写错的流程把机器占住**：一步一个子进程，
/// 三十步已经远超「一个小需求」的形状了。
const MAX_STEPS: usize = 30;

/// 一条工作流。
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    pub id: String,
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub action: String,
    pub input: Map<String, Value>,
}

/// 读一条工作流。**只读形状，不查动作在不在** —— 那是 [`check`] 的事。
///
/// 分成两步是因为两种错的性质不同：形状错是「这份 JSON 写坏了」，
/// 动作缺是「这台机器上没装」。混成一句话会让人往错的方向查。
pub fn parse(v: &Value) -> Result<Flow, String> {
    let o = v.as_object().ok_or("工作流必须是一个对象")?;
    if o.get("spec").and_then(Value::as_str) != Some(SPEC) {
        return Err(format!("只支持 {SPEC}"));
    }
    let text = |k: &str, max: usize| -> Result<String, String> {
        let s = o.get(k).and_then(Value::as_str).unwrap_or("");
        if s.is_empty() || s.chars().count() > max {
            return Err(format!("{k} 必须是 1-{max} 个字符"));
        }
        Ok(s.to_string())
    };
    let raw = o
        .get("steps")
        .and_then(Value::as_array)
        .ok_or("steps 必须是数组")?;
    if raw.is_empty() {
        return Err("steps 不能是空的".into());
    }
    if raw.len() > MAX_STEPS {
        return Err(format!("步数上限 {MAX_STEPS}，这条有 {}", raw.len()));
    }

    let mut steps = Vec::with_capacity(raw.len());
    for (i, s) in raw.iter().enumerate() {
        let so = s.as_object().ok_or(format!("第 {} 步不是对象", i + 1))?;
        let action = so
            .get("action")
            .and_then(Value::as_str)
            .filter(|a| !a.is_empty())
            .ok_or(format!("第 {} 步缺少 action", i + 1))?;
        // 输入允许缺省（有些动作不要参数），但给了就必须是对象 ——
        // 给成数组的话下面取字段会静默拿不到，表现是「动作跑了但什么都没做」
        let input = match so.get("input") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Err(format!("第 {} 步的 input 必须是对象", i + 1)),
        };
        steps.push(Step {
            action: action.to_string(),
            input,
        });
    }
    Ok(Flow {
        id: text("id", 80)?,
        name: text("name", 40)?,
        steps,
    })
}

/// 一条流程能不能在**这台机器上**跑。
///
/// 一次把所有问题都说出来，不是遇到第一个就返回 —— AI 生成的流程往往错好几处，
/// 一次一条会让人来回改五遍。
pub fn check(flow: &Flow) -> Vec<String> {
    let specs = podapp_runtime::manifest::action_specs();
    let mut problems = Vec::new();

    for (i, step) in flow.steps.iter().enumerate() {
        let n = i + 1;

        // **结构性的问题先查，跟动作在不在无关。**
        // 第一版我把这条放在动作查找的 `continue` 后面，于是「动作没装」会把它
        // 一起吞掉 —— 而那恰好是最常见的组合（AI 抄模板，动作名和 `$prev` 一起错），
        // 结果「一次说完所有问题」这个承诺在最需要的时候失效。测试抓到的。
        if i == 0 && mentions(&step.input, PREV) {
            problems.push(format!("第 1 步用了 {PREV}，但它前面没有步骤"));
        }

        let Some(spec) = specs.iter().find(|s| s.id == step.action) else {
            problems.push(format!(
                "第 {n} 步引用了没装的动作 {}（装上提供它的程序舱再跑）",
                step.action
            ));
            continue;
        };
        // 无头跑不了的动作在这条路上根本执行不了。让它跑到那一步再失败，
        // 用户会以为是数据的问题
        if spec.bindings.is_some() && spec.input_schema.is_none() && spec.title.is_empty() {
            problems.push(format!("第 {n} 步的 {} 不能无头执行", step.action));
        }
    }
    problems
}

fn mentions(input: &Map<String, Value>, token: &str) -> bool {
    input.values().any(|v| match v {
        Value::String(s) => s == token,
        Value::Array(a) => a.iter().any(|x| x.as_str() == Some(token)),
        _ => false,
    })
}

/// 把 `$in` / `$prev` 换成真东西。
///
/// 只认**整个值恰好是**这两个记号，不做字符串内插值 ——
/// 内插会让「$prev.png」这种写法看起来能用，而它的含义是模糊的。
fn substitute(input: &Map<String, Value>, seed: Option<&Value>, prev: Option<&Value>) -> Map<String, Value> {
    let swap = |v: &Value| -> Value {
        match v.as_str() {
            Some(IN) => seed.cloned().unwrap_or(Value::Null),
            Some(PREV) => prev.cloned().unwrap_or(Value::Null),
            _ => v.clone(),
        }
    };
    input
        .iter()
        .map(|(k, v)| {
            let out = match v {
                Value::Array(a) => Value::Array(a.iter().map(swap).collect()),
                other => swap(other),
            };
            (k.clone(), out)
        })
        .collect()
}

/// 跑到哪儿了。
#[derive(Debug)]
pub enum Outcome {
    /// 全跑完了。`results` 每步一条。
    Done { results: Vec<Value> },
    /// 停在某一步等确认。**调用方点头之后从 `next` 继续。**
    NeedsConfirm {
        step: usize,
        action: String,
        title: String,
        /// 这一步真正会收到的入参（记号已经换掉了），好让人看清要确认什么
        input: Value,
        results: Vec<Value>,
    },
    /// 某一步失败了。前面几步的结果照样带回来 —— 它们已经产出了东西，
    /// 假装没发生会让人找不到那些产物。
    Failed {
        step: usize,
        action: String,
        error: String,
        results: Vec<Value>,
    },
}

/// 跑一条工作流。
///
/// `from` 是起步的下标（0 开始）。停在确认那一步之后，调用方点头就用
/// `from = step + 1` 再调一次，并把 `results` 传回来接着攒。
///
/// **不跑 [`check`]。** 调用方该先验再跑 —— 把验证塞进这里，就没法在界面上
/// 「先告诉你哪儿不对，你改完再跑」了。
pub fn run(
    flow: &Flow,
    seed: Option<&Value>,
    from: usize,
    mut results: Vec<Value>,
    host: &dyn HostBridge,
    caps: &Capabilities,
) -> Outcome {
    let specs = podapp_runtime::manifest::action_specs();
    // 上一步的产物：接着跑的时候要从传回来的 results 里捡
    let mut prev = results.last().and_then(pick_output);

    for (i, step) in flow.steps.iter().enumerate().skip(from) {
        let input = substitute(&step.input, seed, prev.as_ref());
        let spec = specs.iter().find(|s| s.id == step.action);

        // 要确认的先停。**在跑之前停**，不是跑完再问 ——
        // 跑完再问的那个「确认」已经没有意义了
        if spec.is_some_and(|s| s.confirmation != "never") {
            return Outcome::NeedsConfirm {
                step: i,
                action: step.action.clone(),
                title: spec.map(|s| s.title.clone()).unwrap_or_default(),
                input: Value::Object(input),
                results,
            };
        }

        let inv = Invocation::new(&step.action, Value::Object(input));
        match podapp_runtime::headless::invoke(&inv, host, caps, None) {
            Ok(v) => {
                prev = pick_output(&v);
                results.push(json!({ "step": i, "action": step.action, "result": v }));
            }
            Err(e) => {
                return Outcome::Failed {
                    step: i,
                    action: step.action.clone(),
                    error: e,
                    results,
                }
            }
        }
    }
    Outcome::Done { results }
}

/// 从一步的结果里挑出「能当下一步输入的东西」。
///
/// 优先产物**引用**（路径 / id），不是内容 —— 把像素往下一步传是这个项目
/// 明确不做的事（几 MB base64 在步骤之间来回复制，既慢又会撑爆返回值）。
fn pick_output(step_result: &Value) -> Option<Value> {
    let v = step_result.get("result").unwrap_or(step_result);
    for p in ["/artifact/path", "/artifact/id", "/path", "/file"] {
        if let Some(x) = v.pointer(p) {
            if !x.is_null() {
                return Some(x.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow_json(steps: Value) -> Value {
        json!({ "spec": SPEC, "id": "t.flow", "name": "测试流程", "steps": steps })
    }

    #[test]
    fn a_wrong_shape_is_refused_with_the_step_number() {
        // 「第几步不对」是这里最有用的信息 —— 只说「格式错误」等于让人逐行数
        let e = parse(&flow_json(json!([{ "action": "a.b.c.d" }, { "nope": 1 }]))).unwrap_err();
        assert!(e.contains("第 2 步"), "{e}");

        assert!(parse(&json!({ "spec": "other", "id": "x", "name": "y", "steps": [] })).is_err());
        assert!(parse(&flow_json(json!([]))).unwrap_err().contains("空"));
        // input 给成数组：取字段会静默拿不到，必须在门口拦
        let e = parse(&flow_json(json!([{ "action": "a.b.c.d", "input": [1, 2] }]))).unwrap_err();
        assert!(e.contains("必须是对象"), "{e}");
    }

    #[test]
    fn step_count_is_bounded() {
        let many: Vec<Value> = (0..MAX_STEPS + 1)
            .map(|_| json!({ "action": "a.b.c.d" }))
            .collect();
        assert!(parse(&flow_json(json!(many))).is_err());
    }

    #[test]
    fn input_is_optional_but_a_missing_action_is_not() {
        let f = parse(&flow_json(json!([{ "action": "a.b.c.d" }]))).unwrap();
        assert!(f.steps[0].input.is_empty());
        assert!(parse(&flow_json(json!([{ "input": {} }]))).is_err());
    }

    /// 引用了没装的动作，要在**跑之前**说清，而且把动作 id 说出来。
    #[test]
    fn checking_names_every_missing_action_at_once() {
        let f = parse(&flow_json(json!([
            { "action": "app.nope.one.run" },
            { "action": "app.nope.two.run" },
        ])))
        .unwrap();
        let problems = check(&f);
        // 一次说完，不是遇到第一个就返回 —— AI 生成的流程常常错好几处
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("app.nope.one.run"));
        assert!(problems[1].contains("app.nope.two.run"));
    }

    #[test]
    fn prev_in_the_first_step_is_caught() {
        let f = parse(&flow_json(json!([
            { "action": "app.nope.one.run", "input": { "image": PREV } },
        ])))
        .unwrap();
        assert!(check(&f).iter().any(|p| p.contains("第 1 步用了")));
    }

    #[test]
    fn substitution_replaces_whole_values_only() {
        let mut input = Map::new();
        input.insert("a".into(), json!(IN));
        input.insert("b".into(), json!(PREV));
        input.insert("c".into(), json!("$prev.png"));
        input.insert("d".into(), json!([PREV, "keep"]));
        let out = substitute(&input, Some(&json!("SEED")), Some(&json!("PREV")));

        assert_eq!(out["a"], json!("SEED"));
        assert_eq!(out["b"], json!("PREV"));
        // 不做字符串内插：`$prev.png` 原样留着。看起来能用而含义模糊的写法，
        // 不如让它明确不生效
        assert_eq!(out["c"], json!("$prev.png"));
        assert_eq!(out["d"], json!(["PREV", "keep"]));
    }

    #[test]
    fn output_picking_prefers_references_never_pixels() {
        let r = json!({ "result": { "message": "ok",
            "artifact": { "path": "C:/x/a.png", "id": "art_1" } } });
        assert_eq!(pick_output(&r), Some(json!("C:/x/a.png")));
        // 只有 id 的时候退到 id
        let r2 = json!({ "result": { "artifact": { "id": "art_9" } } });
        assert_eq!(pick_output(&r2), Some(json!("art_9")));
        // 没有任何引用就是 None —— 绝不退化成「把整个结果塞给下一步」
        assert_eq!(pick_output(&json!({ "result": { "message": "ok" } })), None);
    }

    /// 要确认的动作必须**在跑之前**停下。
    ///
    /// 这条是浮舱的立身之本在工作流这条路上的体现：跑完再问的「确认」没有意义。
    ///
    /// # 为什么自己造一个 Pod，而不是从已装的里挑
    ///
    /// 第一版是「找一个声明了 confirmation 的已装动作，找不到就跳过」。
    /// 而官方五个 Pod **全是 `never`** —— 于是这条测试一直走跳过那条路，
    /// 绿着，什么都没验。整个 crate 最要紧的一条断言是假的。
    ///
    /// 现在自己装一个带 `destructive` 动作的 Pod 到隔离目录，并且让那个动作
    /// **落一个哨兵文件**：停下等确认时哨兵必须不存在。
    /// 只断言「返回了 NeedsConfirm」证不了顺序 —— 先跑完再返回也能满足它。
    #[test]
    fn a_step_needing_confirmation_stops_before_running() {
        // 改进程级环境变量，锁在代码里而不是靠 --test-threads=1
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let base = std::env::temp_dir().join(format!("podapp-flow-{}", std::process::id()));
        let src = base.join("src");
        let sentinel = base.join("ran.txt");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(src.join("web")).unwrap();

        std::fs::write(src.join("podapp.json"), FLOWTEST_MANIFEST).unwrap();
        std::fs::write(src.join("action-parity.json"), FLOWTEST_PARITY).unwrap();
        std::fs::write(src.join("web/index.html"), b"<!doctype html><title>t</title>").unwrap();
        // 动作一跑就落哨兵。它写在**入参给的路径**上，因为动作模块的 Node 沙箱
        // 读不到别处 —— 而这里只需要证明「跑过没跑过」
        std::fs::write(
            src.join("web/actions.mjs"),
            br#"export const actions = {
  "app.flowtest.danger.run": async ({ input }, ctx) => {
    await ctx.pod.storage.set("ran", { at: 1 });
    return { ok: true, message: "ran" };
  },
};
export default actions;
"#,
        )
        .unwrap();

        std::env::set_var("PODAPP_HOME", &base);
        std::env::set_var("PODAPP_APPS_ROOT", base.join("apps"));
        let installed = podapp_runtime::install::install_from_path(&src, "test");

        let outcome = installed.as_ref().ok().map(|_| {
            let f = parse(&flow_json(json!([{ "action": "app.flowtest.danger.run" }]))).unwrap();
            // check 得先过：动作装上了、也不是第一步用 $prev
            let problems = check(&f);
            assert!(problems.is_empty(), "{problems:?}");
            run(
                &f,
                None,
                0,
                vec![],
                &podapp_runtime::HeadlessHost::new(),
                &Capabilities::builtin(),
            )
        });

        // 哨兵：storage 落在 <home>/data/<pod>/kv 下
        let kv = base.join("data/org.podapp.test.flowtest/kv/ran.json");
        let ran = kv.exists();
        std::env::remove_var("PODAPP_APPS_ROOT");
        std::env::remove_var("PODAPP_HOME");
        let _ = std::fs::remove_dir_all(&base);
        let _ = sentinel;

        let installed = installed.expect("测试 Pod 该装得上");
        assert_eq!(installed.id, "org.podapp.test.flowtest");
        match outcome.expect("该跑到 run") {
            Outcome::NeedsConfirm { step, action, .. } => {
                assert_eq!(step, 0);
                assert_eq!(action, "app.flowtest.danger.run");
            }
            other => panic!("该停下等确认，却是 {other:?}"),
        }
        // **顺序的证据**：停下的时候动作一步都没跑
        assert!(!ran, "停下等确认之前动作已经跑过了（哨兵存在）");
    }

    const FLOWTEST_MANIFEST: &str = r#"{
  "profile": "podapp/pod@0.1",
  "pod": { "id": "org.podapp.test.flowtest", "slug": "flowtest", "name": "Flow Test",
    "version": "0.1.0", "summary": "one destructive action, for flow tests",
    "author": "PodApp", "license": "Apache-2.0", "locales": ["zh-CN"], "min_host_version": "0.1.0" },
  "action_parity": "./action-parity.json",
  "package": { "kind": "web", "web": { "root": "web", "entry": "index.html", "actions": "actions.mjs" } },
  "ui": { "icon": "lucide:flask", "container": "window", "home_dock": true },
  "permissions": { "ai": { "image_generate": false, "image_edit": false, "chat": false,
      "video_generate": false, "max_calls_per_run": 0 },
    "fs": { "app_data": true, "save_dialog": false, "open_dialog": false } }
}"#;

    const FLOWTEST_PARITY: &str = r#"{
  "spec_version": "0.5.0",
  "application": { "id": "org.podapp.test.flowtest", "name": "Flow Test", "version": "0.1.0" },
  "surfaces": [ { "id": "pod", "kind": "gui", "required_for_parity": true },
                { "id": "cli", "kind": "cli", "required_for_parity": true } ],
  "actions": [ { "id": "app.flowtest.danger.run", "title": "danger",
    "description": "Writes a sentinel so tests can tell whether it ran.",
    "effects": { "class": "write", "risk": "high", "reversible": false,
                 "confirmation": "destructive", "audit_required": false },
    "execution": { "headless": true, "timeout_ms": 5000 },
    "input_schema": { "type": "object", "additionalProperties": false, "properties": {} },
    "bindings": { "pod": "button#danger", "cli": "flowtest danger" } } ]
}"#;
}
