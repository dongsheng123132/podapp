//! 权限 —— 一切默认拒绝。
//!
//! 清单里的 `permissions` 是程序舱**申请**的上限，宿主在**每次调用前**核验。
//! 闸门装在面之下（见 [`crate::headless::dispatch_capability`]）：从 GUI、从无头模块、
//! 还是从 devtools 发起，走的是同一道门。写在规范里的「不许」是对作者的要求，
//! 装在这里的才是让它**做不到**。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AiPerms {
    #[serde(default)]
    pub image_generate: bool,
    #[serde(default)]
    pub image_edit: bool,
    #[serde(default)]
    pub chat: bool,
    #[serde(default)]
    pub video_generate: bool,
    /// 单轮 AI 调用次数硬上限 —— 跑飞的循环不许烧光用户的钱。
    #[serde(default = "def_max_calls")]
    pub max_calls_per_run: u32,
}

fn def_max_calls() -> u32 {
    4
}

/// 手写而不是 `#[derive(Default)]`。
///
/// derive 会给 `max_calls_per_run` 填 0，而 serde 的字段默认是 4 —— 于是「整个 ai 段缺失」
/// 和「ai 段在但没写 max_calls_per_run」会得到两个不同的上限。同一个默认值有两份定义，
/// 就一定会漂移；这里让两条路走同一个 [`def_max_calls`]。
impl Default for AiPerms {
    fn default() -> Self {
        Self {
            image_generate: false,
            image_edit: false,
            chat: false,
            video_generate: false,
            max_calls_per_run: def_max_calls(),
        }
    }
}

fn def_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FsPerms {
    #[serde(default = "def_true")]
    pub app_data: bool,
    #[serde(default)]
    pub save_dialog: bool,
    #[serde(default)]
    pub open_dialog: bool,
}

impl Default for FsPerms {
    fn default() -> Self {
        Self {
            app_data: true,
            save_dialog: false,
            open_dialog: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct NetPerms {
    /// 获准的 https 源。空 = 页面 CSP 只有 `connect-src 'self'`，出不去。
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Perms {
    #[serde(default)]
    pub ai: AiPerms,
    #[serde(default)]
    pub fs: FsPerms,
    #[serde(default)]
    pub net: NetPerms,
    /// 获准调用的**宿主**动作 ID。程序舱自己的动作不填这里。
    #[serde(default)]
    pub host_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    AiImageEdit,
    AiImageGenerate,
    AiChat,
    AiVideoGenerate,
    FsAppData,
    FsSaveDialog,
    FsOpenDialog,
}

/// 这个程序舱申请过这项能力吗。读不出清单一律当没有 —— 失败要往拒绝那边倒。
pub fn permits(pod_id: &str, cap: Cap) -> bool {
    let Ok(p) = crate::manifest::permissions(pod_id) else {
        return false;
    };
    p.has(cap)
}

impl Perms {
    pub fn has(&self, cap: Cap) -> bool {
        match cap {
            Cap::AiImageEdit => self.ai.image_edit,
            Cap::AiImageGenerate => self.ai.image_generate,
            Cap::AiChat => self.ai.chat,
            Cap::AiVideoGenerate => self.ai.video_generate,
            Cap::FsAppData => self.fs.app_data,
            Cap::FsSaveDialog => self.fs.save_dialog,
            Cap::FsOpenDialog => self.fs.open_dialog,
        }
    }

    /// 装包时给用户看的人话清单。**只列会让用户掏东西出来或花钱的**，
    /// 一堆技术名词式的权限行只会训练用户无脑点「同意」。
    pub fn summary(&self) -> Vec<String> {
        let mut v = vec![];
        if self.ai.image_edit {
            v.push("修改图片（消耗额度）".into());
        }
        if self.ai.image_generate {
            v.push("生成图片（消耗额度）".into());
        }
        if self.ai.chat {
            v.push("调用对话模型（消耗额度）".into());
        }
        if self.ai.video_generate {
            v.push("生成视频（消耗额度）".into());
        }
        if self.fs.save_dialog {
            v.push("保存文件到你选的位置".into());
        }
        if self.fs.open_dialog {
            v.push("打开你选的文件".into());
        }
        for h in &self.net.allow {
            v.push(format!("访问 {h}"));
        }
        for h in &self.host_actions {
            v.push(format!("调用宿主动作 {h}"));
        }
        v
    }
}

/// 下发给程序舱文档的 CSP。
///
/// `connect-src 'self'` 是**承重墙**：挡住恶意程序舱把用户的图偷偷发去第三方。
/// 只有 `net.allow` 里获准的 https 源才追加进去。
pub fn csp_for(p: &Perms) -> String {
    let mut connect = String::from("'self'");
    for o in &p.net.allow {
        if o.starts_with("https://") {
            connect.push(' ');
            connect.push_str(o.trim_end_matches('/'));
        }
    }
    format!(
        "default-src 'self'; img-src 'self' data: blob:; media-src 'self' data: blob:; \
         style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; \
         connect-src {connect}; object-src 'none'; base-uri 'none'; frame-ancestors 'self'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_deny_except_own_sandbox() {
        let p = Perms::default();
        assert!(p.has(Cap::FsAppData), "自己的沙箱默认可用");
        for cap in [
            Cap::AiImageEdit,
            Cap::AiImageGenerate,
            Cap::AiChat,
            Cap::AiVideoGenerate,
            Cap::FsSaveDialog,
            Cap::FsOpenDialog,
        ] {
            assert!(!p.has(cap), "{cap:?} 默认必须是拒绝");
        }
        assert!(p.summary().is_empty(), "什么都没申请就不该有权限提示");
    }

    #[test]
    fn csp_keeps_the_load_bearing_wall() {
        let csp = csp_for(&Perms::default());
        assert!(csp.contains("connect-src 'self'"));
        assert!(csp.contains("object-src 'none'"));

        // 获准的源追加进去，未获准的 http 源必须被忽略（降级到明文是攻击面）
        let p = Perms {
            net: NetPerms {
                allow: vec![
                    "https://api.example.com/".into(),
                    "http://evil.example".into(),
                ],
            },
            ..Default::default()
        };
        let csp = csp_for(&p);
        assert!(csp.contains("https://api.example.com"));
        assert!(!csp.contains("evil.example"), "http 源不该进 CSP");
        assert!(!csp.contains("api.example.com/"), "尾斜杠该去掉");
    }
}
