//! 清单方言 —— 同一个内部模型的两种写法。
//!
//! PodApp Protocol 是一份**独立标准**，不是 ActionParity MiniApp Profile 的别名。
//! 但两者描述的是同一件事，所以运行时内部只有一个 [`crate::Manifest`]，
//! 方言层只负责「叫什么名字、放在哪个键下」。
//!
//! 这么做是有代价意识的：两份标准天然会漂移。唯一能防住的机制是**机器强制**的 ——
//! `tests/roundtrip.rs` 拿真实清单跑 MiniApp→内部模型→PodApp→内部模型，
//! 断言两次归一化结果完全一致。那条测试红了，就是两份标准开始分家了。
//!
//! ## [`Dialect::MiniApp`] 不是「U-King 的格式」
//!
//! 它是一份**公开发表的剖面** `action-parity/miniapp@0.1`（规范正文在 uking-miniapp
//! 仓库，Apache-2.0）。U-King 只是恰好实现了它的第一个宿主，就像 PodApp 是第二个。
//!
//! 这个区别不是文字游戏：支持一份开放剖面，和依赖某个产品的私有格式，
//! 是完全不同的两件事。本 crate 对 U-King **没有任何代码依赖**（`Cargo.toml` 里
//! 只有 serde/serde_json/flate2/tar），只有几个字符串常量。
//! 早期这个枚举叫 `UKing`，那个名字让它看起来像依赖，所以改了。
//!
//! 两个方言**共用** `action-parity.json`（动作契约那一半），不做第二种写法。
//! 理由很实在：第三方清单能原样通过上游 ActionParity 官方校验器，
//! 是这套东西的立身之本，为了品牌把它改掉是净亏。

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// PodApp Protocol 0.1 —— `podapp.json`，顶层键 `pod`，包后缀 `.pod`
    PodApp,
    /// ActionParity MiniApp Profile 0.1 —— `uking-app.json`，顶层键 `app`，包后缀 `.ukapp`
    MiniApp,
}

impl Dialect {
    /// 展示清单的文件名。
    pub fn manifest_file(self) -> &'static str {
        match self {
            Dialect::PodApp => "podapp.json",
            Dialect::MiniApp => "uking-app.json",
        }
    }

    /// `profile` 字段的常量值 —— 清单自报家门用的，也是两种方言唯一的硬边界。
    pub fn profile_const(self) -> &'static str {
        match self {
            Dialect::PodApp => "podapp/pod@0.1",
            Dialect::MiniApp => "action-parity/miniapp@0.1",
        }
    }

    /// 身份段所在的顶层键。
    pub fn root_key(self) -> &'static str {
        match self {
            Dialect::PodApp => "pod",
            Dialect::MiniApp => "app",
        }
    }

    /// 可分发包的后缀（不含点）。
    pub fn pkg_ext(self) -> &'static str {
        match self {
            Dialect::PodApp => "pod",
            Dialect::MiniApp => "ukapp",
        }
    }

    /// 人话名字，用在错误消息里。
    pub fn label(self) -> &'static str {
        match self {
            Dialect::PodApp => "程序舱",
            Dialect::MiniApp => "小程序",
        }
    }

    pub fn all() -> [Dialect; 2] {
        [Dialect::PodApp, Dialect::MiniApp]
    }

    /// 认出一个已解包目录用的是哪种方言。
    ///
    /// **两种都在场时报错，不是随便挑一个。** 一个目录里同时躺着 `podapp.json` 和
    /// `uking-app.json`，意味着两份清单可能说着不同的话，而挑哪份都可能是错的 ——
    /// 静默选一个正是「同一事实存在几份就会漂移几份」的开场。
    pub fn detect(dir: &Path) -> Result<Dialect, String> {
        let found: Vec<Dialect> =
            Dialect::all().into_iter().filter(|d| dir.join(d.manifest_file()).exists()).collect();
        match found.as_slice() {
            [one] => Ok(*one),
            [] => Err(format!(
                "目录里既没有 {} 也没有 {}",
                Dialect::PodApp.manifest_file(),
                Dialect::MiniApp.manifest_file()
            )),
            _ => Err(format!(
                "{} 和 {} 同时存在 —— 一个包只能有一份清单，请删掉其中一个",
                Dialect::PodApp.manifest_file(),
                Dialect::MiniApp.manifest_file()
            )),
        }
    }

    /// 由 `profile` 字段值反查方言。
    pub fn from_profile(profile: &str) -> Option<Dialect> {
        Dialect::all().into_iter().find(|d| d.profile_const() == profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_constants_are_distinct() {
        assert_ne!(Dialect::PodApp.profile_const(), Dialect::MiniApp.profile_const());
        assert_ne!(Dialect::PodApp.manifest_file(), Dialect::MiniApp.manifest_file());
        assert_ne!(Dialect::PodApp.pkg_ext(), Dialect::MiniApp.pkg_ext());
    }

    #[test]
    fn from_profile_roundtrips() {
        for d in Dialect::all() {
            assert_eq!(Dialect::from_profile(d.profile_const()), Some(d));
        }
        assert_eq!(Dialect::from_profile("something/else@9"), None);
    }

    #[test]
    fn detect_rejects_ambiguous_dir() {
        let dir = std::env::temp_dir().join(format!("podapp-dialect-{}", crate::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();

        assert!(Dialect::detect(&dir).is_err(), "空目录该报错");

        std::fs::write(dir.join("podapp.json"), "{}").unwrap();
        assert_eq!(Dialect::detect(&dir), Ok(Dialect::PodApp));

        // 两份清单同时在场：必须报错，不许静默挑一个
        std::fs::write(dir.join("uking-app.json"), "{}").unwrap();
        assert!(Dialect::detect(&dir).is_err(), "两份清单同时在场该报错");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
