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

/// 契约要求的图集尺寸：8 列 × 9 行，每格 192×208。
pub const ATLAS_W: u32 = 192 * 8;
pub const ATLAS_H: u32 = 208 * 9;

/// 从**文件头**读图集尺寸。
///
/// 刻意不解码整张图：这里只需要宽高，而引一个图像解码器进来意味着
/// 多一个要跟着升级的攻击面 —— 对一个「读两个整数」的需求不值得。
///
/// PNG 认 IHDR；WebP 三种子格式（VP8X 扩展 / VP8L 无损 / VP8 有损）都认，
/// 因为 hatch-pet 出的是 PNG，而 Nyxie 那类现成宠物出的是 VP8L。
/// 只认一种的后果是「明明是张好图却说格式不对」。
pub fn atlas_size(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| -> Option<u32> {
        Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
    };
    // PNG: 8 字节签名 + 长度 + "IHDR" + 宽 + 高
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16) == Some(b"IHDR") {
        return Some((be32(16)?, be32(20)?));
    }
    if !(b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP")) {
        return None;
    }
    let le24 = |i: usize| -> Option<u32> {
        let s = b.get(i..i + 3)?;
        Some(u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
    };
    let mut i = 12;
    while i + 8 <= b.len() {
        let tag = b.get(i..i + 4)?;
        let size = u32::from_le_bytes(b.get(i + 4..i + 8)?.try_into().ok()?) as usize;
        let data = i + 8;
        match tag {
            // 画布尺寸存的是「实际值 - 1」，少加这个 1 会让 1536 变成 1535，
            // 而错一像素的报错看起来像是图真的不对
            b"VP8X" => return Some((le24(data + 4)? + 1, le24(data + 7)? + 1)),
            b"VP8L" => {
                let bits = u32::from_le_bytes(b.get(data + 1..data + 5)?.try_into().ok()?);
                return Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1));
            }
            b"VP8 " => {
                let w = u16::from_le_bytes(b.get(data + 6..data + 8)?.try_into().ok()?);
                let h = u16::from_le_bytes(b.get(data + 8..data + 10)?.try_into().ok()?);
                return Some((u32::from(w & 0x3FFF), u32::from(h & 0x3FFF)));
            }
            _ => {}
        }
        // RIFF 块长度是奇数时要补一个填充字节，不补的话后面每一块都错位
        i = data + size + (size & 1);
    }
    None
}

/// 这张图能不能当宠物图集。
///
/// **在门口拦住，不要装完再说。** 拖进来一张随手截的图，如果直接装上，
/// 症状是「宠物是一小块糊的东西在抖」—— 人第一反应会怀疑浮舱坏了，
/// 而不是怀疑自己拖错了文件。
pub fn check_atlas(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_SPRITE_BYTES {
        return Err(format!(
            "图集太大（{} MB），上限 {} MB",
            bytes.len() / 1024 / 1024,
            MAX_SPRITE_BYTES / 1024 / 1024
        ));
    }
    // 刻意**不**写 `Some((ATLAS_W, ATLAS_H)) =>`。常量能进模式，但一旦有人把常量
    // 改成小写名，同一行就从「比较」悄悄变成「绑定」，于是任何尺寸都通过 ——
    // 而这道闸门失效是不会报错的。用 if 比较，没有这个歧义。
    let Some((w, h)) = atlas_size(bytes) else {
        return Err("认不出这是 PNG 还是 WebP —— 宠物图集只收这两种".into());
    };
    if (w, h) != (ATLAS_W, ATLAS_H) {
        return Err(format!(
            "图集要 {ATLAS_W}×{ATLAS_H}（8 列 × 9 行，每格 192×208），这张是 {w}×{h}"
        ));
    }
    Ok(())
}

/// 装一只宠物。`src` 可以是**一张图集**，也可以是**一个带 `pet.json` 的目录**。
///
/// 收图集是为了「拖进来就有」：现成宠物（Nyxie 那类）解压出来就是一张 webp，
/// 让人先写一份 `pet.json` 才肯收，等于把门槛抬到没人愿意试。
pub fn install(src: &Path, into: &Path) -> Result<PetInfo, String> {
    let (atlas, name) = if src.is_dir() {
        let pet = read_one(src).ok_or("这个目录里没有能用的 pet.json + 图集")?;
        (pet.sprite, pet.id)
    } else {
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("pet")
            .to_string();
        (src.to_path_buf(), stem)
    };

    let bytes = std::fs::read(&atlas).map_err(|e| format!("读不了图集: {e}"))?;
    check_atlas(&bytes)?;
    let ext = mime_of(&atlas)
        .map(|m| if m == "image/png" { "png" } else { "webp" })
        .ok_or("图集只收 png / webp")?;

    // 文件夹名要能安全地拼回路径。图集叫「我的 宠物!!.webp」是常事，
    // 直接拿去当目录名，之后 find() 会因为 is_safe_id 不认而查不到 ——
    // 而那时现象是「装成功了但列表里没有」，最难查的一类。
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect();
    let id = if is_safe_id(&id) { id } else { "pet".to_string() };

    let dir = into.join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("建不了宠物目录: {e}"))?;
    std::fs::write(dir.join(format!("spritesheet.{ext}")), &bytes)
        .map_err(|e| format!("写不了图集: {e}"))?;
    // 清单照 Codex 的形状写，不加自己的字段 —— 这样这只宠物直接拷进
    // ~/.codex/pets 也能用，两边不会分家
    let manifest = json!({
        "id": id,
        "displayName": name,
        "description": "",
        "spritesheetPath": format!("spritesheet.{ext}"),
    });
    std::fs::write(
        dir.join("pet.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )
    .map_err(|e| format!("写不了 pet.json: {e}"))?;

    read_one(&dir).ok_or_else(|| "装完了却读不回来".to_string())
}

/// 若干个根目录里认得的宠物，按 id 排序（顺序飘的话列表每次打开都在跳）。
///
/// 多个根是因为宠物有两个来源：Codex 自己的 `~/.codex/pets`，和用户拖进浮舱的那些。
/// **同 id 时先出现的根赢** —— 调用方把自己的根放前面，就能覆盖同名的 Codex 宠物。
pub fn list_in(roots: &[PathBuf]) -> Vec<PetInfo> {
    let mut pets: Vec<PetInfo> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            // 目录不存在是常态：没装 Codex、或者还没拖过宠物
            continue;
        };
        for e in entries.flatten().filter(|e| e.path().is_dir()) {
            if let Some(pet) = read_one(&e.path()) {
                if !pets.iter().any(|p| p.id == pet.id) {
                    pets.push(pet);
                }
            }
        }
    }
    pets.sort_by(|a, b| a.id.cmp(&b.id));
    pets
}

/// 本机认得的宠物（只看 Codex 那个根）。
pub fn list() -> Vec<PetInfo> {
    list_in(&[pets_root()])
}

/// 在若干个根里找一只宠物。
pub fn find_in(roots: &[PathBuf], id: &str) -> Option<PetInfo> {
    if !is_safe_id(id) {
        return None;
    }
    roots.iter().find_map(|r| read_one(&r.join(id)))
}

/// 找一只宠物（只看 Codex 那个根）。
pub fn find(id: &str) -> Option<PetInfo> {
    find_in(&[pets_root()], id)
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

    /// 造一个只有文件头的 PNG。`atlas_size` 只读头，所以验尺寸不需要真图 ——
    /// 真造一张 1536×1872 会让这几条测试从毫秒变成秒。
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0]);
        v
    }

    /// VP8L（无损 WebP）的头。Nyxie 那类现成宠物出的就是这个子格式 ——
    /// 只认 PNG 的话，它们会被判成「格式不对」，而图其实完全没问题。
    fn webp_vp8l(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WEBP".to_vec();
        v.extend_from_slice(b"VP8L");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.push(0x2f);
        let bits = (w - 1) | ((h - 1) << 14);
        v.extend_from_slice(&bits.to_le_bytes());
        v.extend_from_slice(&[0; 11]);
        v
    }

    #[test]
    fn reads_atlas_size_from_the_header_of_both_formats() {
        assert_eq!(atlas_size(&png_header(1536, 1872)), Some((1536, 1872)));
        // VP8L 存的是「实际值 - 1」，少加那个 1 会让 1536 变 1535，
        // 而错一像素的报错看起来像图真的不对
        assert_eq!(atlas_size(&webp_vp8l(1536, 1872)), Some((1536, 1872)));
        assert_eq!(atlas_size(b"not an image at all"), None);
        assert_eq!(atlas_size(&[]), None);
    }

    /// 拖错文件要**在门口**说清楚错在哪。只说「格式不对」，人会去转格式，
    /// 而真正的问题是尺寸。
    #[test]
    fn a_wrong_sized_image_is_refused_with_both_numbers() {
        assert!(check_atlas(&png_header(1536, 1872)).is_ok());
        let e = check_atlas(&png_header(800, 600)).unwrap_err();
        assert!(e.contains("1536×1872"), "没说要多大: {e}");
        assert!(e.contains("800×600"), "没说这张是多大: {e}");
        assert!(check_atlas(b"random bytes").unwrap_err().contains("WebP"));
    }

    /// 拖一张图集进来就该有一只宠物，不用先手写 pet.json。
    #[test]
    fn dropping_just_an_atlas_installs_a_pet() {
        sandbox("install", |root| {
            let src = root.parent().unwrap().join("我的 宠物!! v2.png");
            std::fs::write(&src, png_header(1536, 1872)).unwrap();

            let pet = install(&src, root).expect("该装上");
            // 文件夹名必须是能安全拼回路径的形状，否则装完 find() 查不到，
            // 现象是「装成功了但列表里没有」
            assert!(is_safe_id(&pet.id), "id 不安全: {}", pet.id);
            assert_eq!(pet.display_name, "我的 宠物!! v2");
            assert!(find_in(&[root.to_path_buf()], &pet.id).is_some());
            // 清单照 Codex 的形状写，这只宠物拷进 ~/.codex/pets 也该能用
            let m: Value =
                serde_json::from_str(&std::fs::read_to_string(root.join(&pet.id).join("pet.json")).unwrap())
                    .unwrap();
            assert_eq!(m["spritesheetPath"], "spritesheet.png");
        });
    }

    #[test]
    fn a_random_screenshot_is_refused_at_the_door() {
        sandbox("badinstall", |root| {
            let src = root.parent().unwrap().join("screenshot.png");
            std::fs::write(&src, png_header(1920, 1080)).unwrap();
            assert!(install(&src, root).is_err());
            // 拒了就不该留下半只宠物
            assert!(list_in(&[root.to_path_buf()]).is_empty(), "留下了残骸");
        });
    }

    /// 宠物有两个来源（Codex 的和用户拖进来的）。同 id 时靠前的根赢，
    /// 否则「我明明换了一只」会因为 Codex 那边有同名的而看不到效果。
    #[test]
    fn the_first_root_wins_on_a_name_clash() {
        sandbox("roots", |codex| {
            let mine = codex.parent().unwrap().join("mypets");
            std::fs::create_dir_all(&mine).unwrap();
            make_pet(codex, "shared", r#"{"displayName":"Codex 的","spritesheetPath":"s.png"}"#, "s.png");
            make_pet(&mine, "shared", r#"{"displayName":"我的","spritesheetPath":"s.png"}"#, "s.png");

            let pets = list_in(&[mine.clone(), codex.to_path_buf()]);
            assert_eq!(pets.len(), 1, "同 id 该只出现一次");
            assert_eq!(pets[0].display_name, "我的");
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
