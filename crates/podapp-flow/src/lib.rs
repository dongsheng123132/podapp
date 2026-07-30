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

impl Outcome {
    /// 端给界面 / MCP 的形状。
    ///
    /// **放在这里而不是各调用方各写一份**：浮舱和 MCP 都要读它，两处各拼一遍
    /// 迟早会一处认得 `needs_confirm` 另一处认 `needsConfirm`，
    /// 而那种不一致只在「刚好走到确认那一步」时才暴露。
    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Done { results } => json!({ "state": "done", "results": results }),
            Outcome::NeedsConfirm {
                step,
                action,
                title,
                input,
                results,
            } => json!({
                "state": "needs_confirm",
                "step": step, "action": action, "title": title,
                "input": input, "results": results,
                // 点头之后从这儿接着跑，省得调用方自己去 +1（算错就会跳过或重跑一步）
                "resumeFrom": step + 1,
            }),
            Outcome::Failed {
                step,
                action,
                error,
                results,
            } => json!({
                "state": "failed",
                "step": step, "action": action, "error": error, "results": results,
            }),
        }
    }
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
    // 上一步产出了什么。接着跑的时候，`results` 里带着它。
    let mut prev: Option<Value> = results
        .last()
        .and_then(|r| r.get("prev").cloned())
        .filter(|v| !v.is_null());
    let mut prev_count = prev.as_ref().map(|_| 1usize).unwrap_or(0);

    for (i, step) in flow.steps.iter().enumerate().skip(from) {
        // `$prev` 只在上一步**恰好产出一个**产物时有定义。0 个或多个都是没定义的，
        // **这时不猜**：猜一个（比如挑最新的）会让 `nine-grid` 切出 9 张之后
        // 悄悄只把其中一张传下去，而那个错要等到人看结果才发现。
        if i > from.min(i) || i > 0 {
            if mentions(&step.input, PREV) && prev.is_none() {
                return Outcome::Failed {
                    step: i,
                    action: step.action.clone(),
                    error: format!(
                        "第 {} 步要 {PREV}，但上一步产出了 {prev_count} 个产物 —— \
{PREV} 只在恰好 1 个的时候有定义。把它拆成两条流程，\
或者让上一步只产出一个（比如别开 zip）。",
                        i + 1
                    ),
                    results,
                };
            }
        }
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

        // 跑之前记下账本的最新一条，跑完据此算出「这一步产出了什么」
        let marker = newest_artifact();
        let inv = Invocation::new(&step.action, Value::Object(input));
        match podapp_runtime::headless::invoke(&inv, host, caps, None) {
            Ok(v) => {
                let produced = produced_since(marker.as_deref());
                prev_count = produced.len();
                // 恰好一个才给 `$prev`。多个的时候留空，让**下一步**报出一条
                // 说得清的错，而不是在这里挑一个传下去
                prev = if produced.len() == 1 {
                    produced.into_iter().next()
                } else {
                    None
                };
                results.push(json!({
                    "step": i, "action": step.action, "result": v,
                    // 带上「这一步产出了什么」和它算出的 prev，
                    // 好让「点头之后接着跑」那条路拿得回上下文
                    "produced": prev_count,
                    "prev": prev,
                }));
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

/// 产物账本最新那一条的 id。跑一步之前记下来，跑完拿它算出「这一步产出了什么」。
fn newest_artifact() -> Option<String> {
    podapp_runtime::artifacts::list().first().map(|a| a.id.clone())
}

/// 从 `marker` 之后新产出的产物路径（最新的在前）。
///
/// # 为什么查账本，不猜返回值
///
/// 第一版是在返回值里找 `/artifact/path`、`/path` 这些字段。**实测下来一个都对不上**：
/// `nine-grid` 返回的是 `tiles[i].artifact.path` 和 `zip.path`，
/// `annotate` 返回的是 `overlay`。也就是说每个 Pod 用自己的字段名，
/// 而猜字段名的后果是 `$prev` 静默变成 `null` —— 下一步收到 null，
/// 报的错跟真实原因（上一步的字段叫别的名字）隔着好几层。
///
/// 产物账本是**权威**的：产物是经 `artifacts::emit` 落盘的，账本记的就是真实发生过的事，
/// 不依赖任何 Pod 怎么组织自己的返回值。
fn produced_since(marker: Option<&str>) -> Vec<Value> {
    let all = podapp_runtime::artifacts::list();
    let fresh: Vec<_> = match marker {
        None => all,
        Some(id) => all.into_iter().take_while(|a| a.id != id).collect(),
    };
    fresh
        .iter()
        .filter_map(|a| {
            podapp_runtime::artifacts::path_of(&a.id).map(|p| Value::String(p.display().to_string()))
        })
        .collect()
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

    /// 界面和 MCP 读的是同一份形状，而「点头之后从哪儿接着跑」不该让调用方自己算 ——
    /// 算错就会跳过一步或者重跑一步，而两种都不报错。
    #[test]
    fn the_json_shape_says_where_to_resume() {
        let o = Outcome::NeedsConfirm {
            step: 2,
            action: "app.x.y.z".into(),
            title: "危险".into(),
            input: json!({}),
            results: vec![],
        };
        let v = o.to_json();
        assert_eq!(v["state"], "needs_confirm");
        assert_eq!(v["step"], 2);
        assert_eq!(v["resumeFrom"], 3);
        assert_eq!(Outcome::Done { results: vec![] }.to_json()["state"], "done");
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

    /// 「上一步产出了什么」以**产物账本**为准，不是猜返回值里的字段名。
    ///
    /// 这条测试换过一次内容。原来它断言的是「优先 /artifact/path，退到 /path」——
    /// 而实测下来官方 Pod **一个都不长这样**：nine-grid 返回 tiles[i].artifact.path
    /// 和 zip.path，annotate 返回 overlay。也就是说那条测试断言的是一个
    /// 现实中不存在的约定，绿着，而 $prev 在真流程里永远是 null。
    #[test]
    fn what_a_step_produced_comes_from_the_ledger() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let dir = std::env::temp_dir().join(format!("podapp-flow-led-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PODAPP_ARTIFACTS_ROOT", &dir);
        podapp_runtime::artifacts::clear();

        const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        // 账本空的时候，marker 是 None
        assert!(newest_artifact().is_none());
        assert!(produced_since(None).is_empty());

        podapp_runtime::artifacts::emit("p", None, "image", PNG, Some("一")).unwrap();
        let marker = newest_artifact();
        assert!(marker.is_some());
        // marker 之后什么都没产出
        assert!(produced_since(marker.as_deref()).is_empty());

        // 再产两个：从 marker 数应当正好是 2，而且给的是**路径**不是内容
        podapp_runtime::artifacts::emit("p", None, "image", PNG, Some("二")).unwrap();
        podapp_runtime::artifacts::emit("p", None, "image", PNG, Some("三")).unwrap();
        let fresh = produced_since(marker.as_deref());
        assert_eq!(fresh.len(), 2, "{fresh:?}");
        for f in &fresh {
            let s = f.as_str().expect("该是路径字符串");
            assert!(std::path::Path::new(s).exists(), "路径不存在: {s}");
            assert!(!s.contains("iVBORw0"), "把内容当引用传下去了");
        }

        std::env::remove_var("PODAPP_ARTIFACTS_ROOT");
        let _ = std::fs::remove_dir_all(&dir);
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
