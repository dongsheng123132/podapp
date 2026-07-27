//! PodApp 程序舱运行时。
//!
//! 一个程序舱（Pod）= 一份清单 + 一个 GUI 外壳 + 一组 ActionParity 动作。
//! 人点图标、AI 无头调用、影核远端调用走的是**同一条**执行路径 —— 这是本 crate 的立身之本，
//! 不是可选项。两边各写一遍然后祈祷它们一致，第一次改需求就分叉，而且分叉后 GUI 还是对的、
//! AI 那条路悄悄坏掉，没人发现。
//!
//! **两种清单方言，一个内部模型。** `podapp.json`（PodApp Protocol 0.1）与
//! `uking-app.json`（ActionParity MiniApp Profile 0.1）各是一层薄适配器，
//! 归一化成同一个 [`manifest::Manifest`]。`tests/roundtrip.rs` 每次构建都断言两者语义等价 ——
//! 那条测试红了就是两份标准开始分家了，必须当场修，不许 skip。
//!
//! **本 crate 没有网络代码，也从不读凭据文件**，所以它不可能泄露 Key（拿不到的东西泄不了）。
//! AI / 文件 / 宿主动作等能力一律经 [`headless::HostBridge`] 由宿主注入。

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod action_spec;
pub mod artifacts;
pub mod bridge;
pub mod dialect;
pub mod headless;
pub mod image;
pub mod install;
pub mod manifest;
pub mod perms;
pub mod registry;
pub mod selftest;
pub mod serve;

pub use action_spec::ActionSpec;
pub use dialect::Dialect;
pub use headless::{HeadlessHost, HostBridge};
pub use manifest::{Manifest, PodInfo};
pub use perms::{Cap, Perms};

/// 写清单时用的 ActionParity 规范版本（脚手架、自检夹具用它）。
pub const SPEC_VERSION: &str = "0.5.0";

/// 宿主档案 —— 同一份运行时被 PodApp 和 U-King 两个宿主共用，差异全在这里。
///
/// 刻意做成**启动时设一次**而不是到处传参：家目录、桥全局名、自定义 scheme 这些东西
/// 在一个进程里只可能有一个值，做成参数只会让每个调用点都有机会传错。
#[derive(Debug, Clone)]
pub struct HostProfile {
    /// 家目录名，如 `.podapp` / `.uking`
    pub home_dir_name: String,
    /// 环境变量前缀，如 `PODAPP` → `PODAPP_APPS_ROOT`
    pub env_prefix: String,
    /// 注入页面的桥全局名，如 `pod` → `window.pod`
    pub bridge_global: String,
    /// 自定义 scheme，如 `podapp` → `podapp://localhost/...`
    pub scheme: String,
    /// 宿主自己的版本号，用来判断 `min_host_version`
    pub host_version: String,
    /// 写新清单时用哪种方言。**读的时候两种都认**，这里只影响脚手架和自检夹具。
    pub dialect: Dialect,
}

impl Default for HostProfile {
    fn default() -> Self {
        Self {
            home_dir_name: ".podapp".into(),
            env_prefix: "PODAPP".into(),
            bridge_global: "pod".into(),
            scheme: "podapp".into(),
            host_version: env!("CARGO_PKG_VERSION").into(),
            dialect: Dialect::PodApp,
        }
    }
}

impl HostProfile {
    /// U-King 宿主档案 —— 保住 0.9.72 已发版的契约：`.uking` 家目录、`UKING_APPS_ROOT`
    /// 环境变量、`window.uking` 桥、`uking://` scheme。**这些不能改**，改了已装用户的
    /// 小程序全废，而第三方作者根本不知道是谁弄坏的。
    pub fn uking(host_version: impl Into<String>) -> Self {
        Self {
            home_dir_name: ".uking".into(),
            env_prefix: "UKING".into(),
            bridge_global: "uking".into(),
            scheme: "uking".into(),
            host_version: host_version.into(),
            dialect: Dialect::UKing,
        }
    }

    /// PodApp 宿主档案。
    pub fn podapp(host_version: impl Into<String>) -> Self {
        Self { host_version: host_version.into(), ..Default::default() }
    }
}

static PROFILE: OnceLock<HostProfile> = OnceLock::new();

/// 装配宿主档案。宿主启动时调一次；不调就是 PodApp 默认档案。
///
/// 返回 `Err` 表示已经装配过了 —— 中途换档案会让家目录和已打开的资源对不上，
/// 与其半路改不如明确失败。被退回来的档案原样装在 `Box` 里还给调用方。
pub fn init(p: HostProfile) -> Result<(), Box<HostProfile>> {
    PROFILE.set(p).map_err(Box::new)
}

/// 当前宿主档案。
pub fn profile() -> &'static HostProfile {
    PROFILE.get_or_init(HostProfile::default)
}

fn env_var(suffix: &str) -> Option<String> {
    std::env::var(format!("{}_{suffix}", profile().env_prefix)).ok()
}

/// 宿主家目录，如 `~/.podapp`。
pub fn home() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(&profile().home_dir_name)
}

/// 已装程序舱的根目录。
///
/// `<PREFIX>_APPS_ROOT` 能顶掉它 —— 自检靠这个保证**绝不碰用户真实的家目录**，
/// 所以在客户机上跑自检也安全。
pub fn apps_root() -> PathBuf {
    if let Some(p) = env_var("APPS_ROOT") {
        return PathBuf::from(p);
    }
    home().join("apps")
}

/// 程序舱自己的沙箱。**故意放在 `<pod-id>/` 之外**：重装/升级整目录替换，用户数据不跟着没。
pub fn data_dir(id: &str) -> PathBuf {
    apps_root().join(".data").join(id)
}

pub(crate) fn staging_root() -> PathBuf {
    apps_root().join(".staging")
}

pub(crate) fn trash_root() -> PathBuf {
    apps_root().join(".trash")
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 半开区间的版本比较：`a < b`。只看前三段，预发布后缀忽略。
pub(crate) fn version_lt(a: &str, b: &str) -> bool {
    let p = |s: &str| -> Vec<u32> {
        s.split(['.', '-', '+']).take(3).map(|x| x.parse().unwrap_or(0)).collect()
    };
    let (x, y) = (p(a), p(b));
    for i in 0..3 {
        let (u, v) = (*x.get(i).unwrap_or(&0), *y.get(i).unwrap_or(&0));
        if u != v {
            return u < v;
        }
    }
    false
}

/// 把相对路径安全地拼到 `base` 下。任何越界（`..` / 绝对路径 / 盘符 / 符号链接）一律 `None`。
///
/// 这是资源服务和解包共用的**那道门** —— 路径穿越就是从这里漏出去的，
/// 所以两条路必须共用同一份实现，不许各写各的。
pub fn safe_join(base: &std::path::Path, rel: &str) -> Option<PathBuf> {
    if rel.contains('\0') {
        return None;
    }
    let mut out = base.to_path_buf();
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => continue,
            ".." => return None,
            s => {
                if s.contains(':') || is_reserved_name(s) {
                    return None;
                }
                out.push(s);
            }
        }
    }
    // 最终路径必须仍在 base 之内（防符号链接绕过）
    let cb = base.canonicalize().ok()?;
    match out.canonicalize() {
        Ok(co) => co.starts_with(&cb).then_some(out),
        // 还不存在的路径（写入场景）：靠上面的逐段检查兜底
        Err(_) => Some(out),
    }
}

/// Windows 保留设备名。放行它们会让「写一个叫 `NUL.png` 的文件」变成往设备写。
fn is_reserved_name(s: &str) -> bool {
    let stem = s.split('.').next().unwrap_or(s).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_lt("0.9.1", "0.9.2"));
        assert!(version_lt("0.9.72", "0.10.0"));
        assert!(!version_lt("1.0.0", "0.9.99"));
        assert!(!version_lt("0.9.72", "0.9.72"));
        // 预发布后缀不参与比较，不该让 0.9.72-rc1 被判成比 0.9.72 旧
        assert!(!version_lt("0.9.72-rc1", "0.9.72"));
    }

    #[test]
    fn safe_join_blocks_traversal() {
        let base = std::env::temp_dir();
        assert!(safe_join(&base, "../etc/passwd").is_none());
        assert!(safe_join(&base, "a/../../b").is_none());
        assert!(safe_join(&base, "C:/Windows").is_none());
        assert!(safe_join(&base, "NUL.png").is_none());
        assert!(safe_join(&base, "COM1").is_none());
        assert!(safe_join(&base, "web/index.html").is_some());
        // 单纯以保留名开头的正常文件名不该被误杀
        assert!(safe_join(&base, "console.js").is_some());
    }
}
