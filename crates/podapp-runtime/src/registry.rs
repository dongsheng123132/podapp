//! 注册表 —— 已装程序舱的索引。
//!
//! **真相是各程序舱目录里的清单，注册表只是索引/排序缓存。** 两者不一致时以盘上的为准：
//! 盘上有清单但注册表里没有的自动补录，注册表里有但目录没了的丢弃。
//! 反过来做（信索引不信盘）的话，用户手动拷进来一个目录就永远看不见，
//! 而手动删掉一个目录会留下一条点了报错的幽灵记录。

use crate::manifest::PodInfo;
use serde::{Deserialize, Serialize};

fn def_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegEntry {
    pub id: String,
    #[serde(default = "def_true")]
    pub enabled: bool,
    #[serde(default = "def_true")]
    pub pinned_home: bool,
    #[serde(default)]
    pub installed_at: i64,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Registry {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub apps: Vec<RegEntry>,
}

fn path() -> std::path::PathBuf {
    crate::apps_root().join("registry.json")
}

/// 读注册表。**读坏了当空的**，不报错 —— 索引是缓存，缓存坏了该重建，
/// 不该让整个程序舱列表打不开。
pub fn read() -> Registry {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write(r: &Registry) {
    let _ = std::fs::create_dir_all(crate::apps_root());
    if let Ok(s) = serde_json::to_string_pretty(r) {
        let _ = std::fs::write(path(), s);
    }
}

/// 已装程序舱列表。自愈：以盘上的清单为准，顺手把注册表补齐/清干净。
pub fn list() -> Vec<PodInfo> {
    let root = crate::apps_root();
    let mut reg = read();
    let mut out = vec![];
    let mut seen = vec![];

    let lookup = |reg: &Registry, id: &str| {
        reg.apps.iter().find(|e| e.id == id).map(|e| (e.pinned_home, e.enabled))
    };

    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // 点开头的是内部目录（.data / .staging / .trash / .dev），不是程序舱
            if name.starts_with('.') || !e.path().is_dir() {
                continue;
            }
            let hit = lookup(&reg, &name);
            if let Some(i) =
                crate::manifest::info_of(&e.path(), hit.map(|h| h.0), hit.map(|h| h.1), false)
            {
                seen.push(i.id.clone());
                out.push(i);
            }
        }
    }
    // 开发态（脚手架产物）也列出来，但打上 dev 标记且不占首页
    if let Ok(rd) = std::fs::read_dir(root.join(".dev")) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                if let Some(mut i) = crate::manifest::info_of(&e.path(), None, None, true) {
                    i.pinned_home = false;
                    seen.push(i.id.clone());
                    out.push(i);
                }
            }
        }
    }

    let before = reg.apps.len();
    reg.apps.retain(|a| seen.contains(&a.id));
    for i in &out {
        if !i.dev && !reg.apps.iter().any(|a| a.id == i.id) {
            reg.apps.push(RegEntry {
                id: i.id.clone(),
                enabled: true,
                pinned_home: i.home_dock,
                installed_at: crate::now_ms(),
                source: "adopted".into(),
            });
        }
    }
    if reg.apps.len() != before {
        write(&reg);
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn set_pinned_home(id: &str, on: bool) -> Result<(), String> {
    let mut reg = read();
    match reg.apps.iter_mut().find(|a| a.id == id) {
        Some(e) => {
            e.pinned_home = on;
            write(&reg);
            Ok(())
        }
        None => Err(format!("没装这个程序舱: {id}")),
    }
}

pub fn set_enabled(id: &str, on: bool) -> Result<(), String> {
    let mut reg = read();
    match reg.apps.iter_mut().find(|a| a.id == id) {
        Some(e) => {
            e.enabled = on;
            write(&reg);
            Ok(())
        }
        None => Err(format!("没装这个程序舱: {id}")),
    }
}
