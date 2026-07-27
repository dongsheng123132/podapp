//! 动作契约 —— 摊平后喂给宿主动作总线的形状。
//!
//! 这个类型**住在运行时里**，不在宿主里。宿主内置动作和程序舱动作摊进同一张表，
//! 于是 `PodApp.exe action list` 一条命令就能看到全部动作 —— 对 AI 来说，
//! 「哪些是宿主自带的、哪些是装出来的」不该是它需要关心的区别。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSpec {
    pub id: String,
    pub title: String,
    pub description: String,
    /// ActionParity 的 `effects.class`：read / write / external …
    pub effect: String,
    /// `effects.confirmation`：never / destructive / always
    pub confirmation: String,
    pub idempotent: bool,
    pub timeout_ms: u64,
    /// 会不会边跑边报进度。清单里如实声明，调用方据此决定要不要挂监听。
    pub progress_events: bool,
    /// 程序舱动作带自己的契约；宿主内置动作留空，由宿主补默认值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bindings: Option<Value>,
}

impl ActionSpec {
    /// 从 `action-parity.json` 里的一条动作抽出契约。
    ///
    /// 缺字段一律给保守默认值，不报错：上游 ActionParity 迄今每次变更都是**加字段**，
    /// 而消费方只读自己认识的那些。为一个没见过的可选字段判整个包不合法，
    /// 等于上游一发版所有已装程序舱全废。
    pub fn from_parity(a: &Value) -> Option<Self> {
        let id = a.get("id")?.as_str()?.to_string();
        Some(Self {
            id,
            title: a
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            description: a
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            effect: a
                .pointer("/effects/class")
                .and_then(|v| v.as_str())
                .unwrap_or("external")
                .to_string(),
            confirmation: a
                .pointer("/effects/confirmation")
                .and_then(|v| v.as_str())
                .unwrap_or("never")
                .to_string(),
            idempotent: a
                .pointer("/execution/idempotent")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            progress_events: a
                .pointer("/execution/progress_events")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            timeout_ms: a
                .pointer("/execution/timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(600_000),
            input_schema: a.get("input_schema").cloned(),
            output_schema: a.get("output_schema").cloned(),
            bindings: a.get("bindings").cloned(),
        })
    }
}

/// 按动作自己声明的 `input_schema` 校验入参。
///
/// 拒绝非法输入才是把动作交给 agent 时的安全前提 —— **agent 会瞎试**，schema 是唯一的护栏。
/// 覆盖 type/required/additionalProperties/enum/数值边界/字符串长度，够用且零依赖。
pub fn validate_input(schema: &Value, input: &Value, at: &str) -> Result<(), String> {
    let bad = |m: String| Err(format!("invalid_input: {m}"));

    if let Some(t) = schema.get("type").and_then(|v| v.as_str()) {
        let ok = match t {
            "object" => input.is_object(),
            "array" => input.is_array(),
            "string" => input.is_string(),
            "number" => input.is_number(),
            "integer" => input.is_i64() || input.is_u64(),
            "boolean" => input.is_boolean(),
            "null" => input.is_null(),
            _ => true,
        };
        if !ok {
            return bad(format!("{at} 类型应为 {t}"));
        }
    }
    if let Some(e) = schema.get("enum").and_then(|v| v.as_array()) {
        if !e.contains(input) {
            return bad(format!(
                "{at} 必须是 {}",
                serde_json::to_string(e).unwrap_or_default()
            ));
        }
    }
    if let Some(s) = input.as_str() {
        if let Some(n) = schema.get("minLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) < n {
                return bad(format!("{at} 至少 {n} 字"));
            }
        }
        if let Some(n) = schema.get("maxLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) > n {
                return bad(format!("{at} 最多 {n} 字"));
            }
        }
    }
    if let Some(x) = input.as_f64() {
        for (k, ok) in [
            (
                "minimum",
                x >= schema
                    .get("minimum")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::MIN),
            ),
            (
                "maximum",
                x <= schema
                    .get("maximum")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::MAX),
            ),
            (
                "exclusiveMinimum",
                x > schema
                    .get("exclusiveMinimum")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::MIN),
            ),
            (
                "exclusiveMaximum",
                x < schema
                    .get("exclusiveMaximum")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::MAX),
            ),
        ] {
            if schema.get(k).is_some() && !ok {
                return bad(format!("{at} 越界（{k}）"));
            }
        }
    }
    if let Some(o) = input.as_object() {
        if let Some(req) = schema.get("required").and_then(|v| v.as_array()) {
            for r in req.iter().filter_map(|v| v.as_str()) {
                if !o.contains_key(r) {
                    return bad(format!("缺少必填字段 {at}.{r}"));
                }
            }
        }
        let props = schema.get("properties").and_then(|v| v.as_object());
        if schema.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
            for k in o.keys() {
                if !props.map(|p| p.contains_key(k)).unwrap_or(false) {
                    return bad(format!("不认识的字段 {at}.{k}"));
                }
            }
        }
        if let Some(p) = props {
            for (k, v) in o {
                if let Some(s) = p.get(k) {
                    validate_input(s, v, &format!("{at}.{k}"))?;
                }
            }
        }
    }
    if let (Some(arr), Some(items)) = (input.as_array(), schema.get("items")) {
        for (i, v) in arr.iter().enumerate() {
            validate_input(items, v, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object", "additionalProperties": false, "required": ["n"],
            "properties": {
                "n": { "type": "integer", "minimum": 0, "maximum": 10 },
                "tag": { "type": "string", "maxLength": 4 },
                "mode": { "enum": ["a", "b"] }
            }
        })
    }

    #[test]
    fn accepts_valid_input() {
        assert!(validate_input(
            &schema(),
            &json!({ "n": 3, "tag": "abc", "mode": "a" }),
            "input"
        )
        .is_ok());
    }

    #[test]
    fn rejects_the_things_an_agent_gets_wrong() {
        for (bad, why) in [
            (json!({}), "缺必填"),
            (json!({ "n": -1 }), "低于 minimum"),
            (json!({ "n": 11 }), "高于 maximum"),
            (json!({ "n": "3" }), "类型错"),
            (json!({ "n": 1, "extra": 1 }), "未知字段"),
            (json!({ "n": 1, "tag": "toolong" }), "超长"),
            (json!({ "n": 1, "mode": "z" }), "不在 enum 里"),
        ] {
            assert!(
                validate_input(&schema(), &bad, "input").is_err(),
                "该拒绝：{why}"
            );
        }
    }

    #[test]
    fn from_parity_tolerates_missing_optional_fields() {
        // 只有 id 的最小动作也该能读出来 —— 上游加字段不能让老清单失效
        let spec = ActionSpec::from_parity(&json!({ "id": "app.x.y.z" })).unwrap();
        assert_eq!(spec.id, "app.x.y.z");
        assert_eq!(spec.confirmation, "never");
        assert_eq!(spec.timeout_ms, 600_000);
        assert!(ActionSpec::from_parity(&json!({ "title": "无 id" })).is_none());
    }
}
