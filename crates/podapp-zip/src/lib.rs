//! 把若干文件打成一个 `.zip`。
//!
//! # 为什么用「存储」而不压缩
//!
//! 进来的都是 PNG / JPG —— 已经是压缩过的熵，再 deflate 一遍通常省不到 1%，
//! 却要多背一个压缩库、多一条会出错的代码路径。ZIP 的存储模式只需要 CRC32
//! 和几个定长头，所以这里**一个第三方依赖都不引**。
//!
//! 真要压文本的那天再加 deflate，那时它是新增一个 method，不是重写。
//!
//! # 为什么不进 `podapp-runtime`
//!
//! 打包是可插拔能力，不是核心。宿主想要就装上，不想要删掉这个 crate 和
//! `host.rs` 里那一行分发即可 —— 核心的依赖数不该因为它变化。
//!
//! # 为什么不做成桥上的能力
//!
//! 桥上的能力（`image.*` 那类）对所有程序舱一律开放。打包会读**别的产物**，
//! 该由清单逐条申报（`permissions.host_actions`），装包时明明白白列给用户看。

mod host;
pub use host::host_action;

use std::sync::OnceLock;

/// 一个待打包的条目。`name` 是 zip 里的路径，不是磁盘路径。
pub struct Entry {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Entry {
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }
}

/// 打成 zip。`ts_ms` 是写进每个条目的修改时间（Unix 毫秒）——
/// **由调用方传入而不是这里取当前时间**，否则同样的输入每次打出的字节都不同，
/// 测试就只能断言「大概对」。
pub fn write(entries: &[Entry], ts_ms: i64) -> Result<Vec<u8>, String> {
    if entries.is_empty() {
        return Err("invalid_input: 没有要打包的东西".into());
    }

    let mut seen = std::collections::HashSet::new();
    for e in entries {
        check_name(&e.name)?;
        if !seen.insert(e.name.as_str()) {
            return Err(format!("invalid_input: 重名条目 {}", e.name));
        }
    }

    let (date, time) = dos_datetime(ts_ms);
    let mut out: Vec<u8> = Vec::new();
    // (name, crc, size, 本地头偏移)
    let mut central: Vec<(&str, u32, u32, u32)> = Vec::new();

    for e in entries {
        let size =
            u32::try_from(e.bytes.len()).map_err(|_| format!("单个文件超过 4GB：{}", e.name))?;
        let offset = u32::try_from(out.len()).map_err(|_| "打包结果超过 4GB".to_string())?;
        let crc = crc32(&e.bytes);
        let name = e.name.as_bytes();

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes()); // 本地文件头签名
        out.extend_from_slice(&20u16.to_le_bytes()); // 解压所需版本 2.0
        out.extend_from_slice(&UTF8_FLAG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // method = 0（存储）
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes()); // 存储模式下压缩前后一样大
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // 无 extra
        out.extend_from_slice(name);
        out.extend_from_slice(&e.bytes);

        central.push((&e.name, crc, size, offset));
    }

    let cd_start = u32::try_from(out.len()).map_err(|_| "打包结果超过 4GB".to_string())?;
    for (name, crc, size, offset) in &central {
        let name = name.as_bytes();
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes()); // 中央目录项签名
        out.extend_from_slice(&20u16.to_le_bytes()); // 由 2.0 版本创建
        out.extend_from_slice(&20u16.to_le_bytes()); // 需要 2.0 版本
        out.extend_from_slice(&UTF8_FLAG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(&0u16.to_le_bytes()); // 注释
        out.extend_from_slice(&0u16.to_le_bytes()); // 起始磁盘号
        out.extend_from_slice(&0u16.to_le_bytes()); // 内部属性
        out.extend_from_slice(&0u32.to_le_bytes()); // 外部属性
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(name);
    }
    let cd_size = out.len() as u32 - cd_start;

    let n = u16::try_from(central.len()).map_err(|_| "条目超过 65535 个".to_string())?;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes()); // 中央目录结束记录
    out.extend_from_slice(&0u16.to_le_bytes()); // 本磁盘号
    out.extend_from_slice(&0u16.to_le_bytes()); // 中央目录起始磁盘号
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // 无注释

    Ok(out)
}

/// 第 11 位 = 文件名是 UTF-8。**不设这一位，中文名在资源管理器里就是乱码** ——
/// 早期 ZIP 没有编码字段，解压方只能按本机代码页猜。
const UTF8_FLAG: u16 = 0x0800;

/// zip 里的路径是解压方会照着**创建文件**的字符串。放进去之前必须挡住
/// 目录穿越和绝对路径 —— 否则一个精心构造的名字能让解压覆盖 zip 之外的文件。
/// 这道闸在这里，而不是指望每个调用方都记得洗一遍。
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("invalid_input: 条目名是空的".into());
    }
    if name.len() > 512 {
        return Err(format!("invalid_input: 条目名过长：{name}"));
    }
    if name.starts_with('/') || name.starts_with('\\') || name.contains(':') {
        return Err(format!("invalid_input: 条目名不能是绝对路径：{name}"));
    }
    if name.split(['/', '\\']).any(|seg| seg == ".." || seg == ".") {
        return Err(format!("invalid_input: 条目名不能含 .. 或 .：{name}"));
    }
    if name.contains('\0') || name.chars().any(|c| c.is_control()) {
        return Err(format!("invalid_input: 条目名含控制字符：{name}"));
    }
    Ok(())
}

fn crc_table() -> &'static [u32; 256] {
    static T: OnceLock<[u32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    })
}

fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = t[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Unix 毫秒 → (DOS 日期, DOS 时间)。
///
/// DOS 的纪元是 1980，秒只有 2 秒精度。早于 1980 的时间没法表示，
/// 一律夹到 1980-01-01 —— 写个负数进去，某些解压器会直接判定文件损坏。
fn dos_datetime(ts_ms: i64) -> (u16, u16) {
    let secs = ts_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    if y < 1980 {
        return (0x0021, 0); // 1980-01-01 00:00:00
    }
    let date = (((y - 1980) as u16) << 9) | ((m as u16) << 5) | d as u16;
    let time = (((tod / 3600) as u16) << 11)
        | ((((tod % 3600) / 60) as u16) << 5)
        | ((tod % 60) / 2) as u16;
    (date, time)
}

/// 天数（自 1970-01-01）→ 公历年月日。Howard Hinnant 的 `civil_from_days`。
/// 自己算是为了不引 chrono —— 这个 crate 的零依赖比省这 15 行值钱。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC32 的标准测试向量。这个值错了，包看着能打开、解出来的文件是坏的 ——
    /// 而且多数解压器只在校验时才发现，那时用户已经把图发出去了。
    #[test]
    fn crc32_matches_the_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn layout_is_a_well_formed_zip() {
        let z = write(&[Entry::new("a.txt", b"hello".to_vec())], 0).unwrap();
        assert_eq!(
            &z[..4],
            &0x0403_4b50u32.to_le_bytes(),
            "开头必须是本地文件头"
        );
        assert_eq!(
            &z[z.len() - 22..z.len() - 18],
            &0x0605_4b50u32.to_le_bytes(),
            "结尾必须是 EOCD"
        );
        // 存储模式：原始字节原样出现在包里
        assert!(z.windows(5).any(|w| w == b"hello"));
    }

    /// 同样的输入必须打出同样的字节。做不到的话，「产物变了没」这个问题
    /// 就永远只能靠人眼看。
    #[test]
    fn output_is_deterministic() {
        let mk = || write(&[Entry::new("a.png", vec![1, 2, 3])], 1_785_000_000_000).unwrap();
        assert_eq!(mk(), mk());
    }

    /// 时间戳只影响 DOS 时间那两个字段，不该改变结构长度。
    #[test]
    fn timestamp_only_shifts_the_date_fields() {
        let a = write(&[Entry::new("a.png", vec![9])], 0).unwrap();
        let b = write(&[Entry::new("a.png", vec![9])], 1_785_000_000_000).unwrap();
        assert_eq!(a.len(), b.len());
        assert_ne!(a, b);
    }

    /// 早于 DOS 纪元的时间要夹住，不能写出负年份。
    #[test]
    fn pre_1980_clamps_instead_of_underflowing() {
        let z = write(&[Entry::new("a", vec![1])], -3_000_000_000_000).unwrap();
        let date = u16::from_le_bytes([z[12], z[13]]);
        assert_eq!(date, 0x0021, "应夹到 1980-01-01");
    }

    /// 名字是解压方照着建文件的字符串 —— 穿越和绝对路径必须在这里就被挡住。
    #[test]
    fn dangerous_names_are_refused() {
        for bad in [
            "../escape.png",
            "a/../../escape.png",
            "/abs.png",
            "\\abs.png",
            "C:/abs.png",
            "with\nnewline",
            "",
        ] {
            let r = write(&[Entry::new(bad, vec![1])], 0);
            assert!(r.is_err(), "这个名字不该被放行：{bad:?}");
        }
        // 子目录本身是合法的
        assert!(write(&[Entry::new("tiles/01.png", vec![1])], 0).is_ok());
    }

    #[test]
    fn duplicate_names_are_refused() {
        let r = write(
            &[Entry::new("a.png", vec![1]), Entry::new("a.png", vec![2])],
            0,
        );
        assert!(r.is_err(), "重名会让解压方悄悄少一个文件");
    }

    #[test]
    fn empty_input_is_refused() {
        assert!(write(&[], 0).is_err());
    }

    /// 中文名必须带 UTF-8 标志位，否则资源管理器里是乱码。
    #[test]
    fn chinese_names_carry_the_utf8_flag() {
        let z = write(&[Entry::new("第一张.png", vec![1])], 0).unwrap();
        let flags = u16::from_le_bytes([z[6], z[7]]);
        assert_eq!(flags & UTF8_FLAG, UTF8_FLAG);
        assert!(z
            .windows(9)
            .any(|w| w == "第一张.png".as_bytes()[..9].to_vec().as_slice()));
    }

    #[test]
    fn entry_count_and_offsets_line_up() {
        let entries = [
            Entry::new("01.png", vec![1; 100]),
            Entry::new("02.png", vec![2; 200]),
            Entry::new("03.png", vec![3; 300]),
        ];
        let z = write(&entries, 0).unwrap();
        let eocd = z.len() - 22;
        assert_eq!(u16::from_le_bytes([z[eocd + 10], z[eocd + 11]]), 3);
        let cd_off =
            u32::from_le_bytes([z[eocd + 16], z[eocd + 17], z[eocd + 18], z[eocd + 19]]) as usize;
        assert_eq!(&z[cd_off..cd_off + 4], &0x0201_4b50u32.to_le_bytes());
    }
}
