//! `host.zip.pack` 的实现。
//!
//! 放在这个 crate 而不是浮舱里，是因为**同一个动作只能有一份实现**：
//! 浮舱要用它，无头测试也要用它。写在浮舱里，测试就只能自己再拼一个「差不多的」，
//! 而那份副本会先漂移、再悄悄和真实行为分家 —— 分家之后测试还是绿的。

use podapp_runtime::artifacts;
use serde_json::{json, Value};

/// 宿主动作分发。宿主照 `podapp_codex::host_action` 那个写法接一行即可。
pub fn host_action(id: &str, input: Value) -> Result<Value, String> {
    match id {
        "host.zip.pack" => pack(input),
        other => Err(format!("capability_unavailable: 没有这个宿主动作 {other}")),
    }
}

/// 把收件箱里已有的若干产物打成一个 zip，作为新产物交付。
///
/// 入参：
/// - `artifacts`: `["art_x", ...]` 必填，要打包的产物 id
/// - `names`: `["01.png", ...]` 可选，zip 内的文件名；缺省是 `序号.原后缀`
/// - `label`: 可选，只进收件箱那行说明，给用户一句人话
///
/// **只收产物 id，不收磁盘路径。** 收件箱里装的是程序舱自己交出来的东西，
/// 打包它们不算越权；而让程序舱指定任意路径去打包，等于绕开沙箱读用户的盘 ——
/// 这道闸比「打包能用」重要得多。
fn pack(input: Value) -> Result<Value, String> {
    let ids = input
        .get("artifacts")
        .and_then(|v| v.as_array())
        .ok_or("invalid_input: 缺少 artifacts（产物 id 数组）")?;
    if ids.is_empty() {
        return Err("invalid_input: artifacts 是空的".into());
    }
    // 上限是防手滑，不是防攻击 —— 真正兜底的是产物 64MB 的上限。
    if ids.len() > 512 {
        return Err("invalid_input: 一次最多打包 512 个产物".into());
    }
    let names = input.get("names").and_then(|v| v.as_array());
    let inbox = artifacts::list();

    let mut entries = Vec::with_capacity(ids.len());
    for (i, id) in ids.iter().enumerate() {
        let id = id
            .as_str()
            .ok_or("invalid_input: artifacts 里必须都是字符串 id")?;
        let meta = inbox
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| format!("not_found: 收件箱里没有产物 {id}"))?;
        let bytes = artifacts::read_bytes(id)
            .ok_or_else(|| format!("not_found: 产物 {id} 的文件读不出来（可能已被清理）"))?;
        let ext = meta.file.rsplit('.').next().unwrap_or("bin");
        let name = names
            .and_then(|n| n.get(i))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("{:02}.{ext}", i + 1));
        entries.push(crate::Entry::new(name, bytes));
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let zip = crate::write(&entries, ts)?;

    let label = input
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("打包");
    let msg = format!("{label} · {} 个文件 · {}", entries.len(), human(zip.len()));
    // `HostBridge::host_action` 只给动作 id 和入参，拿不到调用方是哪个程序舱，
    // 所以 source 记成动作本身。**记错来源比不记更坏** —— 收件箱里那行字是
    // 用户判断「这东西哪来的」的唯一依据。
    let art = artifacts::emit_bytes(
        "host.zip.pack",
        Some("host.zip.pack"),
        "archive",
        &zip,
        Some(&msg),
    )?;
    let path = artifacts::path_of(&art.id).map(|p| p.display().to_string());

    Ok(json!({
        "ok": true,
        "count": entries.len(),
        "bytes": art.bytes,
        "artifact": { "id": art.id, "kind": art.kind, "bytes": art.bytes, "path": path },
        "message": msg,
    }))
}

fn human(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PODAPP_ARTIFACTS_ROOT` 是进程级的，几个测试同时改会互相踩。
    /// 锁在代码里而不是靠 `--test-threads=1` —— 需要特殊参数才能过的测试是陷阱，
    /// 下一个人照常跑 `cargo test` 就看到红的。
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 把收件箱引到临时目录，绝不碰用户真实的 `~/.podapp/artifacts`。
    fn sandbox(tag: &str, f: impl FnOnce()) {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("podapp-zip-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PODAPP_ARTIFACTS_ROOT", &dir);
        artifacts::clear();
        f();
        std::env::remove_var("PODAPP_ARTIFACTS_ROOT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 1×1 的合法 PNG。用真 PNG 是为了让后缀嗅探走通（存成 .png 而不是 .bin）。
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    fn emit(msg: &str) -> String {
        artifacts::emit("test.pod", None, "image", PNG, Some(msg))
            .unwrap()
            .id
    }

    #[test]
    fn packs_real_artifacts_into_a_zip_artifact() {
        sandbox("pack", || {
            let ids = vec![emit("第一张"), emit("第二张")];
            let out = host_action(
                "host.zip.pack",
                json!({ "artifacts": ids, "label": "九宫格" }),
            )
            .unwrap();

            assert_eq!(out["ok"], true);
            assert_eq!(out["count"], 2);

            // 交出来的必须是**引用**：返回值里出现 base64 就是把内容漏进去了
            let path = out["artifact"]["path"].as_str().expect("要给出落盘路径");
            assert!(!out.to_string().contains("iVBORw0"), "返回值里不能夹带内容");
            // 后缀必须是 .zip —— 存成 .bin，用户下下来双击打不开
            assert!(path.ends_with(".zip"), "产物后缀应为 zip，实际 {path}");

            let bytes = std::fs::read(path).unwrap();
            assert_eq!(&bytes[..4], b"PK\x03\x04");
            let eocd = bytes.len() - 22;
            assert_eq!(u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]), 2);
            assert!(bytes.windows(6).any(|w| w == b"01.png"));
            assert!(bytes.windows(6).any(|w| w == b"02.png"));
        });
    }

    #[test]
    fn caller_supplied_names_win() {
        sandbox("names", || {
            let ids = vec![emit("a")];
            let out = host_action(
                "host.zip.pack",
                json!({ "artifacts": ids, "names": ["第一行.png"] }),
            )
            .unwrap();
            let bytes = std::fs::read(out["artifact"]["path"].as_str().unwrap()).unwrap();
            let want = "第一行.png".as_bytes();
            assert!(
                bytes.windows(want.len()).any(|w| w == want),
                "中文名应原样进包"
            );
        });
    }

    /// 只认收件箱里的产物 id。这条守的是沙箱边界，不是易用性。
    #[test]
    fn refuses_anything_that_is_not_a_known_artifact_id() {
        sandbox("deny", || {
            for bad in [
                json!({ "artifacts": ["art_nope"] }),
                json!({ "artifacts": [r"C:\Windows\System32\config\SAM"] }),
                json!({ "artifacts": ["../../secret.txt"] }),
            ] {
                assert!(
                    host_action("host.zip.pack", bad.clone()).is_err(),
                    "不该放行：{bad}"
                );
            }
        });
    }

    #[test]
    fn refuses_malformed_input() {
        sandbox("bad", || {
            assert!(
                host_action("host.zip.pack", json!({})).is_err(),
                "缺 artifacts"
            );
            assert!(
                host_action("host.zip.pack", json!({ "artifacts": [] })).is_err(),
                "空数组"
            );
            assert!(
                host_action("host.zip.pack", json!({ "artifacts": [1, 2] })).is_err(),
                "非字符串 id"
            );
            let many: Vec<String> = (0..513).map(|i| format!("art_{i}")).collect();
            assert!(
                host_action("host.zip.pack", json!({ "artifacts": many })).is_err(),
                "超过上限"
            );
        });
    }

    #[test]
    fn unknown_host_action_is_refused() {
        assert!(host_action("host.zip.nope", json!({})).is_err());
    }
}
