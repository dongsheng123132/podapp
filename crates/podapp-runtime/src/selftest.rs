//! 自检 —— 装 → 列 → 跑 → 卸，全程在临时目录里。
//!
//! **绝不碰用户真实的家目录**（靠 `<PREFIX>_APPS_ROOT` 顶掉），所以在客户机上跑也安全。
//! 夹具内置在代码里，不依赖任何外部仓库 —— 否则这个自检在客户机上就是废的。
//!
//! 每一条断言都对应一个**踩过或差点踩的坑**，不是为了好看：
//! 桥没注入表现为「点了没反应」，路径穿越表现为什么都不表现，
//! 沙箱失效表现为一切正常直到密钥被读走。这些都查不出来，所以每次构建都验一遍。
//!
//! 两种方言各跑一遍完整闭环 —— 「两份标准一个运行时」不能只在清单层成立。

use crate::dialect::Dialect;
use serde_json::{json, Value};
use std::path::Path;

/// 写一份最小可用的夹具到 `dir`，用指定方言。
fn write_fixture(dir: &Path, d: Dialect) -> std::io::Result<()> {
    let web = dir.join("web");
    std::fs::create_dir_all(&web)?;
    let host_ver = &crate::profile().host_version;

    let mut manifest = serde_json::Map::new();
    manifest.insert("profile".into(), json!(d.profile_const()));
    manifest.insert(
        d.root_key().into(),
        json!({
            "id": "org.podapp.selftest", "slug": "selftest", "name": "自检",
            "version": "0.1.0", "min_host_version": host_ver
        }),
    );
    manifest.insert(
        "package".into(),
        json!({ "kind": "web", "web": { "root": "web", "entry": "index.html", "actions": "actions.mjs" } }),
    );
    manifest.insert("ui".into(), json!({ "icon": "lucide:check", "home_dock": false }));
    manifest.insert("permissions".into(), json!({}));
    std::fs::write(dir.join(d.manifest_file()), Value::Object(manifest).to_string())?;

    std::fs::write(
        dir.join("action-parity.json"),
        json!({
            "spec_version": crate::SPEC_VERSION,
            "application": { "id": "org.podapp.selftest", "name": "自检", "version": "0.1.0" },
            "surfaces": [
                { "id": "pod", "kind": "gui", "required_for_parity": true },
                { "id": "cli", "kind": "cli", "required_for_parity": true }
            ],
            "actions": [{
                "id": "app.selftest.echo.run",
                "title": "Echo",
                "description": "Return the input doubled. Pure computation, no side effects.",
                "input_schema": { "type": "object", "additionalProperties": false,
                                  "required": ["n"], "properties": { "n": { "type": "integer", "minimum": 0 } } },
                "output_schema": { "type": "object" },
                "effects": { "class": "read", "risk": "low", "reversible": true,
                             "confirmation": "never", "audit_required": false },
                "execution": { "headless": true, "idempotent": true, "cancellable": false, "timeout_ms": 5000 },
                "bindings": [
                    { "surface": "pod", "target": "pod-rpc:action/app.selftest.echo.run" },
                    { "surface": "cli", "target": "cli:action run app.selftest.echo.run --json" }
                ]
            }]
        })
        .to_string(),
    )?;

    std::fs::write(web.join("index.html"), "<!doctype html><meta charset=utf-8><body>selftest")?;
    std::fs::write(
        web.join("actions.mjs"),
        // 夹具同时扮演「正常程序舱」和「恶意程序舱」：
        // 既走一遍 artifact.emit（正常路必须通），也试着越狱读用户目录、起子进程（必须被拦）。
        // 沙箱不能只写在规范里 —— 这两条断言就是让它每次构建都被证明一次。
        //
        // 顺带验证桥的两个名字指向同一个对象：照着 `pod.*` 写的程序舱要能在 U-King 里跑，
        // 而已发版的 `uking.*` 程序舱要能继续跑。
        concat!(
            "const PNG1x1 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';\n",
            "export default { 'app.selftest.echo.run': async (i, ctx) => {\n",
            "  const aliasOk = ctx.pod === ctx.uking;\n",
            "  const a = await ctx.pod.artifact.emit({ kind: 'image', data: PNG1x1, message: 'selftest artifact' });\n",
            "  let escape = 'NOT_BLOCKED';\n",
            "  try {\n",
            "    const fs = await import('node:fs');\n",
            "    const home = process.env.USERPROFILE || process.env.HOME;\n",
            "    fs.readdirSync(home);\n",
            "  } catch (e) { escape = 'BLOCKED:' + (e.code || e.message); }\n",
            "  let spawn = 'NOT_BLOCKED';\n",
            "  try { const cp = await import('node:child_process'); cp.execSync('whoami'); }\n",
            "  catch (e) { spawn = 'BLOCKED:' + (e.code || e.message); }\n",
            "  return { ok: true, doubled: i.n * 2, artifact: a, escape, spawn, aliasOk };\n",
            "} };\n"
        ),
    )?;
    Ok(())
}

/// 跑一遍自检。返回失败项数（0 = 全过），可直接当进程退出码用。
pub fn run() -> i32 {
    let sandbox = std::env::temp_dir().join(format!("podapp-selftest-{}", crate::now_ms()));
    let p = crate::profile();
    std::env::set_var(format!("{}_APPS_ROOT", p.env_prefix), &sandbox);
    std::env::set_var(format!("{}_ARTIFACTS_ROOT", p.env_prefix), sandbox.join("home"));

    let mut fail = 0;
    {
        let mut step = |ok: bool, what: &str, detail: String| {
            println!(
                "{} {what}{}",
                if ok { "PASS" } else { "FAIL" },
                if detail.is_empty() { String::new() } else { format!("  ({detail})") }
            );
            if !ok {
                fail += 1;
            }
        };

        for d in Dialect::all() {
            println!("\n── 方言：{} ({}) ──", d.profile_const(), d.manifest_file());

            let src = sandbox.join(format!(".fixture-{}", d.pkg_ext()));
            let _ = std::fs::remove_dir_all(&src);
            step(write_fixture(&src, d).is_ok(), "生成夹具", String::new());

            match crate::install::install_from_path(&src, "selftest") {
                Ok(i) => step(true, "安装", format!("{} v{}", i.id, i.version)),
                Err(e) => step(false, "安装", e),
            }
            let listed = crate::registry::list();
            step(
                listed.iter().any(|i| i.id == "org.podapp.selftest"),
                "列表可见",
                format!("{} 个", listed.len()),
            );
            step(
                crate::manifest::action_specs().iter().any(|a| a.id == "app.selftest.echo.run"),
                "动作已并入宿主动作总线",
                String::new(),
            );

            match crate::headless::run_action("app.selftest.echo.run", json!({ "n": 21 })) {
                Ok(v) => {
                    let got = v.get("doubled").and_then(|x| x.as_i64());
                    step(got == Some(42), "无头执行", format!("doubled={got:?}"));
                    step(
                        v.get("aliasOk").and_then(|x| x.as_bool()) == Some(true),
                        "桥的两个名字指向同一个对象",
                        String::new(),
                    );
                    // 产物必须是「引用」而不是像素：有 id/path，且**不含** base64 载荷
                    let art = v.get("artifact").cloned().unwrap_or(Value::Null);
                    let has_ref = art.get("id").and_then(|x| x.as_str()).is_some()
                        && art.get("path").and_then(|x| x.as_str()).is_some();
                    let no_payload = !art.to_string().contains("iVBORw0");
                    step(has_ref && no_payload, "产物返回引用而非像素", format!("{art}"));
                    step(
                        crate::artifacts::list().iter().any(|a| a.source == "org.podapp.selftest"),
                        "产物进了收件箱",
                        format!("{} 件", crate::artifacts::list().len()),
                    );
                    step(
                        crate::artifacts::unseen_count() > 0,
                        "未读计数可用（角标靠它）",
                        String::new(),
                    );
                    // 沙箱：动作模块必须读不到用户目录、起不了子进程。
                    // 这两条一旦回归，「程序舱永远拿不到密钥」就是假话。
                    let esc = v.get("escape").and_then(|x| x.as_str()).unwrap_or("?");
                    let spw = v.get("spawn").and_then(|x| x.as_str()).unwrap_or("?");
                    step(esc.starts_with("BLOCKED"), "沙箱挡住读用户目录", esc.to_string());
                    step(spw.starts_with("BLOCKED"), "沙箱挡住起子进程", spw.to_string());
                }
                Err(e) => step(false, "无头执行", e),
            }
            step(
                crate::headless::run_action("app.selftest.echo.run", json!({ "n": -1 })).is_err(),
                "非法入参被拒",
                String::new(),
            );
            step(
                crate::headless::run_action("app.selftest.echo.run", json!({ "bad": 1 })).is_err(),
                "未知字段被拒",
                String::new(),
            );

            let served = crate::serve::serve("org.podapp.selftest", "../../../podapp.json");
            step(served.status != 200, "路径穿越被挡", format!("status={}", served.status));

            // 入口页 + 桥注入。这条一旦坏掉，每个程序舱打开都没有 window.pod，
            // 而界面上只表现为「点了没反应」—— 查起来极费劲。
            let entry = crate::serve::serve("org.podapp.selftest", "");
            step(
                entry.status == 200 && entry.mime.starts_with("text/html"),
                "入口页可服务",
                format!("status={} mime={}", entry.status, entry.mime),
            );
            let injected = crate::bridge::inject(&entry.body, "org.podapp.selftest");
            let html = String::from_utf8_lossy(&injected);
            let hits = html.matches("bridge.js").count();
            step(
                hits == 1 && html.contains("data-pod=\"org.podapp.selftest\""),
                "桥注入入口页且只注一次",
                format!("{hits} 次"),
            );
            let js = crate::bridge::script();
            step(
                js.contains("window.pod") && js.contains("artifact") && js.contains("/rpc/"),
                "桥脚本内容完整",
                String::new(),
            );

            // CSP 的承重墙：没有 connect-src 'self'，恶意程序舱能把用户的图外传第三方
            let csp = crate::manifest::permissions("org.podapp.selftest")
                .map(|p| crate::perms::csp_for(&p))
                .unwrap_or_default();
            step(
                csp.contains("connect-src 'self'") && csp.contains("object-src 'none'"),
                "CSP 含 connect-src 'self'",
                String::new(),
            );

            match crate::install::uninstall("org.podapp.selftest", true) {
                Ok(()) => step(
                    !crate::registry::list().iter().any(|i| i.id == "org.podapp.selftest"),
                    "卸载",
                    String::new(),
                ),
                Err(e) => step(false, "卸载", e),
            }
        }
    }

    let _ = std::fs::remove_dir_all(&sandbox);
    println!("\n{}", if fail == 0 { "全部通过".to_string() } else { format!("{fail} 项失败") });
    fail
}
