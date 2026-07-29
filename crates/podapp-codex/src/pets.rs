//! 只读访问 Codex 的自定义宠物。
//!
//! # 为什么照抄 Codex 的目录，而不是自己定一份
//!
//! Codex 的宠物契约已经定死了：`${CODEX_HOME:-~/.codex}/pets/<名字>/` 下面一个
//! `pet.json` 加一张图集，图集是 8 列 × 9 行、每格 192×208、透明底。
//!
//! 自己再定一份格式，等于让做宠物的人二选一 —— 而这件事上二选一没有赢家：
//! 同一只宠物在 Codex app 里能动、在浮舱里不能动，用户只会觉得浮舱坏了。
//! **照抄那份契约，`hatch-pet` 生成的宠物就直接能用**，一行转换代码都不用写。
//!
//! # 为什么皮肤里只能写文件夹名，不能写路径
//!
//! 皮肤是**能从陌生人手里导入的 JSON**，浮舱的皮肤面板上明写着
//! 「仅颜色、标记和圆角，不执行第三方代码」。要是皮肤能带一个任意文件路径，
//! 那句承诺当场就破了 —— 一份来路不明的 JSON 就能让浮舱去读本机任意文件。
//!
//! 所以皮肤只写**宠物文件夹名**，路径由这里拼；拼完还要再验一次落点仍在宠物根目录内
//! （`..` 能穿过朴素的字符检查，但穿不过 canonicalize 之后的前缀比对）。
//!
//! # 对上游内部结构的态度
//!
//! 跟 [`crate::sessions_root`] 一样：只依赖很稳的两件事 —— 宠物一个文件夹一只、
//! 文件夹里有 `pet.json`。认不出的文件夹**跳过而不是报错**，因为用户完全可能
//! 往那儿放别的东西，而「有一个文件夹不认识」不该让整个宠物列表变空。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 图集最大字节数。
///
/// 按契约算，1536×1872 的 PNG 撑死几 MB。给到 32 MiB 已经很宽了 ——
/// 留这道闸不是防正常宠物，是防「有人往那儿放了个 2GB 的文件」：
/// WebView 会真的去读它，然后浮舱卡死，而现象跟宠物一点关系都看不出来。
const MAX_SPRITE_BYTES: u64 = 32 * 1024 * 1024;

/// 宠物根目录。`CODEX_HOME` 顶掉它 —— 测试靠这个绝不碰用户真实的宠物。
pub fn pets_root() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_HOME") {
        return PathBuf::from(p).join("pets");
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".codex").join("pets")
}

/// 一只宠物。
#[derive(Debug, Clone)]
pub struct PetInfo {
    /// 文件夹名。Codex 就是拿它当 id 的，这里不另发明一个。
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub sprite: PathBuf,
    pub bytes: u64,
}

impl PetInfo {
    /// 给界面看的形状。**不含图集字节** —— 一张几 MB 的图 base64 进列表，
    /// 会让「有几只宠物」这个问题变成一次几十 MB 的 IPC。要图去 `/pet/<id>/sprite` 拿。
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "displayName": self.display_name,
            "description": self.description,
            "bytes": self.bytes,
        })
    }
}

/// 这个文件夹名能不能安全地拼进路径。
///
/// 白名单而不是黑名单：黑名单永远漏一个（`..`、`.`、`C:`、`\\?\`、尾随空格、
/// Windows 保留设备名……），白名单只需要判断「我认得的字符」。
fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn mime_of(p: &Path) -> Option<&'static str> {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        // 契约写的是 "PNG or WebP"，就只认这两种。
        // 多认一种就是多一种「Codex 里不动但浮舱里动」的不一致。
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

/// 读一只宠物的清单。认不出就返回 `None`（调用方跳过它，不报错）。
fn read_one(dir: &Path) -> Option<PetInfo> {
    let id = dir.file_name()?.to_str()?.to_string();
    if !is_safe_id(&id) {
        return None;
    }
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("pet.json")).ok()?).ok()?;

    // spritesheetPath 是清单里的相对路径。它同样来自磁盘上一份可能是别人给的文件，
    // 所以走跟文件夹名一样的验证：拼完必须仍在这只宠物的目录里。
    let rel = manifest
        .get("spritesheetPath")
        .and_then(|v| v.as_str())
        .unwrap_or("spritesheet.webp");
    let sprite = safe_join(dir, rel)?;
    if mime_of(&sprite).is_none() {
        return None;
    }
    let bytes = std::fs::metadata(&sprite).ok()?.len();
    if bytes == 0 || bytes > MAX_SPRITE_BYTES {
        return None;
    }

    Some(PetInfo {
        display_name: manifest
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .chars()
            .take(48)
            .collect(),
        description: manifest
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect(),
        id,
        sprite,
        bytes,
    })
}

/// 把相对路径拼进目录，并确认**落点仍在目录之内**。
///
/// 光看字符串不够：`a/../../b` 里每一段都是合法字符，拼出来却在外面。
/// 所以拼完 canonicalize 再比前缀 —— 这一步同时也解开了软链接。
fn safe_join(dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.contains(':') || rel.starts_with('/') || rel.starts_with('\\') {
        return None;
    }
    let joined = dir.join(rel);
    let real = std::fs::canonicalize(&joined).ok()?;
    let root = std::fs::canonicalize(dir).ok()?;
    real.starts_with(&root).then_some(real)
}

/// 本机认得的宠物，按名字排序（顺序飘的话皮肤列表每次打开都在跳）。
pub fn list() -> Vec<PetInfo> {
    let Ok(entries) = std::fs::read_dir(pets_root()) else {
        // 没装 Codex、或者一只宠物都没做过 —— 这是常态不是错误
        return vec![];
    };
    let mut pets: Vec<PetInfo> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| read_one(&e.path()))
        .collect();
    pets.sort_by(|a, b| a.id.cmp(&b.id));
    pets
}

/// 找一只宠物。
pub fn find(id: &str) -> Option<PetInfo> {
    if !is_safe_id(id) {
        return None;
    }
    read_one(&pets_root().join(id))
}

/// 图集字节 + MIME。给 `podapp://` 那条路直接端给 WebView。
///
/// **不在 Rust 这边解码。** WebView2 自己就认 PNG 和 WebP，
/// 引一个图像解码器进来只是多一份要跟着升级的攻击面。
pub fn sprite_bytes(id: &str) -> Result<(Vec<u8>, &'static str), String> {
    let pet = find(id).ok_or_else(|| format!("没有这只宠物: {id}"))?;
    let mime = mime_of(&pet.sprite).ok_or("图集格式不认（只认 png / webp）")?;
    let bytes = std::fs::read(&pet.sprite).map_err(|e| format!("读不了图集: {e}"))?;
    Ok((bytes, mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这些测试共享进程级的 `CODEX_HOME`，同时跑会互相踩。锁在代码里而不是靠
    /// `--test-threads=1` —— 需要特殊参数才能过的测试是陷阱。
    ///
    /// 用的是 crate 根上那把**共用**的锁，不是自己新开一把：会话那批测试也在改
    /// 同一个环境变量，各锁各的等于没锁。
    use crate::CODEX_HOME_LOCK as SERIAL;

    /// 1×1 的透明 PNG。真图集是 1536×1872，但这些测试验的是**路径和边界**，
    /// 不是像素 —— 用真尺寸只会让测试慢。
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn sandbox(tag: &str, f: impl FnOnce(&Path)) {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::temp_dir().join(format!("podapp-pets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("pets")).unwrap();
        std::env::set_var("CODEX_HOME", &home);
        f(&home.join("pets"));
        std::env::remove_var("CODEX_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    fn make_pet(root: &Path, id: &str, manifest: &str, sprite_name: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pet.json"), manifest).unwrap();
        if !sprite_name.is_empty() {
            std::fs::write(dir.join(sprite_name), PNG).unwrap();
        }
    }

    #[test]
    fn reads_a_pet_written_the_way_codex_writes_it() {
        sandbox("ok", |root| {
            make_pet(
                root,
                "ember",
                r#"{"id":"ember","displayName":"小炭","description":"一只会喘气的炭火","spritesheetPath":"spritesheet.png"}"#,
                "spritesheet.png",
            );
            let pets = list();
            assert_eq!(pets.len(), 1);
            assert_eq!(pets[0].id, "ember");
            assert_eq!(pets[0].display_name, "小炭");
            assert_eq!(pets[0].bytes, PNG.len() as u64);

            let (bytes, mime) = sprite_bytes("ember").unwrap();
            assert_eq!(mime, "image/png");
            assert_eq!(bytes, PNG);
        });
    }

    /// 没装 Codex、或者一只宠物都没做过 —— 这是常态。
    /// 报错的话皮肤面板每次打开都会红一下，而用户什么都没做错。
    #[test]
    fn no_pets_is_not_an_error() {
        sandbox("empty", |_| {
            assert!(list().is_empty());
            assert!(find("nope").is_none());
            assert!(sprite_bytes("nope").is_err());
        });
    }

    /// 一个文件夹不认识，不该让整个列表变空 —— 用户完全可能往那儿放别的东西。
    #[test]
    fn one_bad_folder_does_not_hide_the_good_ones() {
        sandbox("mixed", |root| {
            make_pet(root, "good", r#"{"spritesheetPath":"s.png"}"#, "s.png");
            // 清单不是 JSON
            make_pet(root, "broken", "{ 这不是 json", "s.png");
            // 清单指的图不存在
            make_pet(root, "missing", r#"{"spritesheetPath":"s.png"}"#, "");
            // 格式不在契约里
            make_pet(root, "wrongfmt", r#"{"spritesheetPath":"s.bmp"}"#, "s.bmp");
            std::fs::write(root.join("一个文件.txt"), "x").unwrap();

            let ids: Vec<String> = list().into_iter().map(|p| p.id).collect();
            assert_eq!(ids, vec!["good"]);
        });
    }

    /// **皮肤是能从陌生人手里导入的 JSON。** 它写的那个名字要是能穿出宠物目录，
    /// 「不执行第三方代码」那句承诺就只剩字面意思了。
    #[test]
    fn a_pet_name_can_never_escape_the_pets_folder() {
        sandbox("escape", |root| {
            make_pet(root, "good", r#"{"spritesheetPath":"s.png"}"#, "s.png");
            // 目录外真的放一个能读的文件，确认拿不到的原因是被拦住，不是文件不存在
            std::fs::write(root.parent().unwrap().join("secret.png"), PNG).unwrap();

            for evil in [
                "..",
                ".",
                "../secret",
                "..\\secret",
                "a/b",
                "a\\b",
                "C:\\Windows",
                "",
                ".hidden",
            ] {
                assert!(find(evil).is_none(), "{evil:?} 不该被认成宠物名");
                assert!(sprite_bytes(evil).is_err(), "{evil:?} 不该读得出图集");
            }
        });
    }

    /// 文件夹名安全，不代表清单里那行相对路径也安全 —— 它同样来自磁盘上一份
    /// 可能是别人给的文件。两处都要验，验一处等于没验。
    #[test]
    fn the_manifest_cannot_point_outside_the_pet_folder_either() {
        sandbox("relescape", |root| {
            std::fs::write(root.join("outside.png"), PNG).unwrap();
            make_pet(root, "sneaky", r#"{"spritesheetPath":"../outside.png"}"#, "");
            assert!(find("sneaky").is_none(), "清单里的 ../ 穿出去了");

            make_pet(root, "absolute", r#"{"spritesheetPath":"C:\\x.png"}"#, "");
            assert!(find("absolute").is_none(), "清单里的绝对路径被接受了");
        });
    }

    /// 列表顺序要稳。飘的话皮肤面板每次打开条目都在跳，而这种「偶尔不一样」
    /// 最难被当成 bug 报上来。
    #[test]
    fn the_list_comes_back_in_a_stable_order() {
        sandbox("order", |root| {
            for id in ["zeta", "alpha", "mid"] {
                make_pet(root, id, r#"{"spritesheetPath":"s.png"}"#, "s.png");
            }
            let ids: Vec<String> = list().into_iter().map(|p| p.id).collect();
            assert_eq!(ids, vec!["alpha", "mid", "zeta"]);
        });
    }
}
