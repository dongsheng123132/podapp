//! 防漂移测试 —— 两份标准，一个语义。
//!
//! PodApp Protocol 和 ActionParity MiniApp Profile 是两份独立标准，描述的却是同一件事。
//! 两份标准天然会漂移，而漂移是**无声**的：某天有人往 podapp.json 里加了个字段，
//! uking-app.json 那边没加，运行时读 uking 包时把它读成默认值 —— 界面上一切正常，
//! 只有那个字段悄悄失效。
//!
//! 唯一能防住的机制是机器强制的，就是本文件。**这条测试红了，就是两份标准开始分家了。**
//! 修的办法是让两边继续等价，不是给测试加豁免、也不是 `#[ignore]`。
//!
//! 夹具用的是真实清单形状（照 U-King 已发版的 imagefix / resize / idcard 三个小程序），
//! 不是为通过测试而裁剪过的最小样本 —— 那样只能证明最小样本等价。

#![allow(clippy::bool_assert_comparison)]

use podapp_runtime::dialect::Dialect;
use podapp_runtime::manifest::Manifest;
use serde_json::{json, Value};

/// 一份形状完整的清单：所有可选段都填上，包括 annotation、quick_actions、
/// 各类权限，以及方言层不认识的 `market` / `$schema`。
fn full_manifest(d: Dialect) -> Value {
    let mut o = serde_json::Map::new();
    o.insert(
        "$schema".into(),
        json!("https://example.invalid/schema.json"),
    );
    o.insert("profile".into(), json!(d.profile_const()));
    o.insert(
        d.root_key().into(),
        json!({
            "id": "org.podapp.image.nine-grid",
            "slug": "nine-grid",
            "name": "九宫格切图",
            "version": "0.1.0",
            "summary": "把一张图切成九张",
            "description": "自动识别 3×3，支持边距与批量导出。",
            "author": "PodApp",
            "homepage": "https://podapp.net/pods/nine-grid",
            "license": "MIT",
            "locales": ["zh-CN", "en"],
            "min_host_version": "0.1.0"
        }),
    );
    o.insert("action_parity".into(), json!("./action-parity.json"));
    o.insert(
        "package".into(),
        json!({
            "kind": "web",
            "web": { "root": "web", "entry": "index.html", "actions": "actions.mjs" }
        }),
    );
    o.insert(
        "ui".into(),
        json!({
            "icon": "icon.png",
            "accent": "#7c5cff",
            "container": "both",
            "window": { "width": 1100, "height": 760, "resizable": true },
            "home_dock": true,
            "quick_actions": [
                { "action": "app.nine-grid.image.split", "label": "切九宫格", "icon": "lucide:grid-3x3" }
            ],
            "annotation": {
                "kind": "rect",
                "target_field": "region",
                "action": "app.nine-grid.image.split",
                "prompt": "框住要切的区域",
                "image_field": "image"
            }
        }),
    );
    o.insert(
        "permissions".into(),
        json!({
            "ai": { "image_edit": true, "image_generate": false, "chat": false,
                    "video_generate": false, "max_calls_per_run": 3 },
            "fs": { "app_data": true, "save_dialog": true, "open_dialog": true },
            "net": { "allow": ["https://api.example.com"] },
            "host_actions": ["host.zip.pack"]
        }),
    );
    o.insert(
        "market".into(),
        json!({ "category": "image", "tags": ["切图", "九宫格"] }),
    );
    Value::Object(o)
}

/// 最小清单：只有必填项。默认值的处理在两个方言之间也必须一致。
fn minimal_manifest(d: Dialect) -> Value {
    json!({
        "profile": d.profile_const(),
        d.root_key(): { "id": "org.podapp.hello", "slug": "hello", "name": "你好", "version": "0.1.0" },
        "package": { "kind": "web" },
        "ui": { "icon": "lucide:box" }
    })
}

#[test]
fn every_dialect_reads_into_the_same_model() {
    for build in [full_manifest as fn(Dialect) -> Value, minimal_manifest] {
        let from_pod = Manifest::from_json(&build(Dialect::PodApp)).expect("podapp 方言该能读");
        let from_uking = Manifest::from_json(&build(Dialect::MiniApp)).expect("uking 方言该能读");

        // 除了「我是哪种方言」这一个字段，两边归一化后必须逐字段相同
        assert_eq!(from_pod.dialect, Dialect::PodApp);
        assert_eq!(from_uking.dialect, Dialect::MiniApp);
        assert_eq!(
            from_pod.translated(Dialect::MiniApp),
            from_uking,
            "两种方言读出来的语义不一致 —— 两份标准开始分家了"
        );
    }
}

#[test]
fn translating_between_dialects_loses_nothing() {
    // uking → 内部模型 → podapp → 内部模型：两次归一化必须完全一致
    for build in [full_manifest as fn(Dialect) -> Value, minimal_manifest] {
        let original = Manifest::from_json(&build(Dialect::MiniApp)).unwrap();
        let as_podapp_json = original.to_json(Dialect::PodApp);
        let back = Manifest::from_json(&as_podapp_json).unwrap();

        assert_eq!(back.dialect, Dialect::PodApp, "转换后 profile 必须换过来");
        assert_eq!(
            original.translated(Dialect::PodApp),
            back,
            "跨方言转换掉了信息 —— 这是漂移的第一步"
        );

        // 反向也要成立
        let round = Manifest::from_json(&back.to_json(Dialect::MiniApp)).unwrap();
        assert_eq!(original, round, "转回来对不上");
    }
}

#[test]
fn unknown_top_level_keys_survive_a_round_trip() {
    // 上游加字段不该在一次读写往返里被我们悄悄吃掉 —— 悄悄吃掉比报错更难查
    let m = Manifest::from_json(&full_manifest(Dialect::MiniApp)).unwrap();
    let out = m.to_json(Dialect::PodApp);
    assert_eq!(out["market"]["category"], "image", "market 段被吃掉了");
    assert_eq!(
        out["$schema"], "https://example.invalid/schema.json",
        "$schema 被吃掉了"
    );
}

#[test]
fn the_identity_section_moves_to_the_right_key() {
    let m = Manifest::from_json(&full_manifest(Dialect::MiniApp)).unwrap();

    let as_pod = m.to_json(Dialect::PodApp);
    assert_eq!(as_pod["profile"], Dialect::PodApp.profile_const());
    assert_eq!(as_pod["pod"]["id"], "org.podapp.image.nine-grid");
    assert!(as_pod.get("app").is_none(), "podapp 方言不该还留着 app 段");

    let as_uking = m.to_json(Dialect::MiniApp);
    assert_eq!(as_uking["profile"], Dialect::MiniApp.profile_const());
    assert_eq!(as_uking["app"]["id"], "org.podapp.image.nine-grid");
    assert!(as_uking.get("pod").is_none(), "uking 方言不该还留着 pod 段");
}

#[test]
fn a_manifest_without_a_known_profile_is_rejected() {
    // 认不出方言时必须报错。猜一个默认方言等于把「这份清单说的是什么」变成运行时抛硬币。
    let mut v = full_manifest(Dialect::PodApp);
    v["profile"] = json!("someone-else/format@2");
    let e = Manifest::from_json(&v).unwrap_err();
    assert!(e.contains("不认识的 profile"), "实际: {e}");

    v.as_object_mut().unwrap().remove("profile");
    assert!(Manifest::from_json(&v).is_err(), "缺 profile 也必须拒绝");
}

#[test]
fn identity_under_the_wrong_key_is_rejected_not_defaulted() {
    // podapp 方言的清单把身份写在 app 下 —— 必须报错，不能悄悄读成空
    let mut v = full_manifest(Dialect::PodApp);
    let ident = v["pod"].clone();
    let o = v.as_object_mut().unwrap();
    o.remove("pod");
    o.insert("app".into(), ident);

    let e = Manifest::from_json(&v).unwrap_err();
    assert!(
        e.contains("\"pod\""),
        "错误信息该点明 pod 段缺失，实际: {e}"
    );
}

#[test]
fn permissions_default_to_deny_in_both_dialects() {
    // 缺 permissions 段时的默认值必须一致 —— 一边默认拒绝一边默认放行是最危险的漂移
    for d in Dialect::all() {
        let m = Manifest::from_json(&minimal_manifest(d)).unwrap();
        assert!(!m.permissions.ai.image_edit, "{d:?}: AI 能力必须默认关");
        assert!(!m.permissions.fs.save_dialog, "{d:?}: 另存为必须默认关");
        assert!(m.permissions.fs.app_data, "{d:?}: 自己的沙箱默认可用");
        assert!(m.permissions.net.allow.is_empty(), "{d:?}: 默认出不了网");
        assert!(
            m.permissions.host_actions.is_empty(),
            "{d:?}: 默认调不了宿主动作"
        );
        assert_eq!(
            m.permissions.ai.max_calls_per_run, 4,
            "{d:?}: AI 次数上限默认值不一致"
        );
    }
}
