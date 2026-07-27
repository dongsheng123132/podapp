//! 向后兼容测试 —— 已经发出去的东西不许弄坏。
//!
//! `tests/fixtures/` 里是 U-King 0.9.72 随 exe 一起发货的三个小程序的**真实清单**
//! （从 `u-king简化版/src-tauri/apps/` 原样拷来，只留两份 JSON）。
//!
//! 为什么要拿真清单当夹具：自己捏的夹具只能证明「我以为的形状」能过。这三份是已经装在
//! 用户机器上的形状 —— M4 把 U-King 切到本 crate 时，它们必须一字不改地继续可读。
//! 这条测试红了，意味着升级会让已装用户的小程序全废，而他们不知道是谁弄坏的。
//!
//! 更新夹具的唯一正当理由是上游真的发了新版清单；**不是**为了让测试变绿。

use podapp_runtime::dialect::Dialect;
use podapp_runtime::manifest::Manifest;
use serde_json::Value;
use std::path::PathBuf;

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

const SHIPPED: [&str; 3] = ["idcard", "imagefix", "resize"];

fn load(name: &str) -> Manifest {
    let p = fixture_dir(name).join("uking-app.json");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {p:?}: {e}"));
    let v: Value = serde_json::from_str(&text).expect("夹具不是合法 JSON");
    Manifest::from_json(&v).unwrap_or_else(|e| panic!("{name} 的真实清单读不出来: {e}"))
}

#[test]
fn every_shipped_miniapp_still_parses() {
    for name in SHIPPED {
        let m = load(name);
        assert_eq!(m.dialect, Dialect::MiniApp);
        assert!(!m.ident.id.is_empty(), "{name}: id 空了");
        assert!(!m.ident.slug.is_empty(), "{name}: slug 空了");
        assert_eq!(m.package.kind, "web", "{name}: 三个内置的都是 web 形态");
        assert!(m.package.web.is_some(), "{name}: package.web 丢了");
    }
}

#[test]
fn shipped_manifests_survive_translation_to_podapp_and_back() {
    // 真实清单跨方言往返 —— 比合成夹具更能暴露我们没想到的字段
    for name in SHIPPED {
        let original = load(name);
        let back = Manifest::from_json(&original.to_json(Dialect::PodApp))
            .unwrap_or_else(|e| panic!("{name} 转成 podapp 方言后读不回来: {e}"));
        assert_eq!(
            original.translated(Dialect::PodApp),
            back,
            "{name}: 真实清单跨方言往返掉了信息"
        );

        let round = Manifest::from_json(&back.to_json(Dialect::MiniApp)).unwrap();
        assert_eq!(original, round, "{name}: 转回 uking 方言后对不上");
    }
}

#[test]
fn imagefix_keeps_the_details_that_make_it_work() {
    // imagefix 是三个里字段最全的：quick_actions、annotation、AI 权限、独立窗口尺寸。
    // 逐个点名，因为「能解析」和「解析对了」是两回事 —— 少读一个字段照样解析成功。
    let m = load("imagefix");
    assert_eq!(m.ident.id, "org.uking.app.imagefix");
    assert_eq!(m.ident.slug, "imagefix");

    assert!(
        m.permissions.ai.image_edit,
        "去水印靠 AI 改图，这个权限不能丢"
    );
    assert!(!m.permissions.ai.image_generate, "它没申请生成图片，别多给");
    assert_eq!(m.permissions.ai.max_calls_per_run, 3, "额度上限被读错了");
    assert!(m.permissions.fs.save_dialog && m.permissions.fs.open_dialog);
    assert!(m.permissions.net.allow.is_empty(), "它不该能出网");

    assert_eq!(m.ui.container, "both");
    assert_eq!(m.ui.quick_actions.len(), 2, "去水印 + 改文字两个快捷入口");
    let an =
        m.ui.annotation
            .as_ref()
            .expect("annotation 段丢了 —— 拖框去水印全靠它");
    assert_eq!(an.kind, "rect");
    assert_eq!(an.target_field, "region");
    assert_eq!(an.image_field.as_deref(), Some("image"));

    // 方言层不认识的 market 段也要原样留着
    assert_eq!(
        m.extra.get("market").and_then(|v| v.get("category")),
        Some(&serde_json::json!("image"))
    );
}

#[test]
fn shipped_action_ids_stay_in_their_namespace() {
    // 动作 ID 是外部契约：AI、CLI、影核都拿它调。改了等于毁约。
    for name in SHIPPED {
        let m = load(name);
        let p = fixture_dir(name).join("action-parity.json");
        let parity: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();

        assert_eq!(
            parity["application"]["id"],
            m.ident.id.as_str(),
            "{name}: 两份清单身份对不上"
        );
        assert_eq!(parity["application"]["version"], m.ident.version.as_str());

        let actions = parity["actions"].as_array().expect("actions 该是数组");
        assert!(!actions.is_empty(), "{name}: 一个动作都没有");
        for a in actions {
            let id = a["id"].as_str().unwrap();
            assert!(
                id.starts_with(&format!("app.{}.", m.ident.slug)),
                "{name}: 动作 {id} 跑出命名空间了"
            );
        }
    }
}
