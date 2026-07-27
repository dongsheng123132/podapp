//! 清单 —— 归一化模型、两种方言的读写、以及不信任打包方的那一整套校验。
//!
//! **宿主不信任打包方。** 安装和预览都要过 [`load_dir`] 这一关：路径、命名空间、
//! 引用完整性、无头实现是否真的存在，一条都不能省。第三方包会带着善意的错误来，
//! 也会带着恶意的错误来，而这里是唯一能拦住两者的地方。

use crate::dialect::Dialect;
use crate::perms::Perms;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// ───────────────────────────── 归一化模型 ─────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct PodIdent {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locales: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_host_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct WebPkg {
    #[serde(default = "def_web_root")]
    pub root: String,
    #[serde(default = "def_web_entry")]
    pub entry: String,
    /// 动作模块（不碰 DOM）。任何 headless 动作都要靠它 —— 没有它 parity 就是空话。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<String>,
}

fn def_web_root() -> String {
    "web".into()
}
fn def_web_entry() -> String {
    "index.html".into()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct ScriptPkg {
    #[serde(default)]
    pub skill_dir: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct NativePkg {
    #[serde(default)]
    pub exe: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_cli: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Package {
    /// web | script | native
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WebPkg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptPkg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativePkg>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct QuickAction {
    pub action: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// 标注即输入 —— 宿主采集矩形/多边形，程序舱只收坐标。
///
/// 存在的理由：让每个程序舱自己写一遍选取界面，等于让每个作者重新踩一遍
/// 「坐标系是原图的还是显示的」这个坑，而且每个都会踩得不一样。
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Annotation {
    /// rect | poly | point
    pub kind: String,
    /// 采集到的坐标写进入参的哪个字段
    pub target_field: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_field: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Ui {
    pub icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default = "def_container")]
    pub container: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Value>,
    #[serde(default = "def_true")]
    pub home_dock: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quick_actions: Vec<QuickAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<Annotation>,
}

fn def_container() -> String {
    "embed".into()
}
fn def_true() -> bool {
    true
}

/// 一份归一化清单。两种方言读进来长得一模一样 —— 这是防漂移的地基。
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// 它是从哪种方言读出来的。只影响写回和错误措辞，不影响任何行为。
    pub dialect: Dialect,
    pub ident: PodIdent,
    pub action_parity: Option<String>,
    pub package: Package,
    pub ui: Ui,
    pub permissions: Perms,
    /// 方言层不认识的顶层键（如 `market`、`$schema`）。原样留着，写回时带上 ——
    /// 上游加字段不该在一次读写往返里被我们悄悄吃掉。
    pub extra: serde_json::Map<String, Value>,
}

/// 前端卡片用的轻量信息（不含 web 载荷）。
#[derive(Debug, Clone, Serialize)]
pub struct PodInfo {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub kind: String,
    pub icon: String,
    pub accent: Option<String>,
    pub container: String,
    pub home_dock: bool,
    pub pinned_home: bool,
    pub enabled: bool,
    pub actions: Vec<String>,
    pub quick_actions: Vec<Value>,
    pub permissions: Vec<String>,
    pub dev: bool,
}

// ───────────────────────────── 方言读写 ─────────────────────────────

/// 已知的顶层键。除此之外的一律进 `extra` 原样保留。
const KNOWN_KEYS: &[&str] = &["profile", "action_parity", "package", "ui", "permissions"];

impl Manifest {
    /// 从一份清单 JSON 解析。方言由 `profile` 字段自报，不靠文件名猜。
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let profile = v.get("profile").and_then(|x| x.as_str()).unwrap_or("");
        let dialect = Dialect::from_profile(profile).ok_or_else(|| {
            format!(
                "不认识的 profile: {profile:?}（应为 {} 或 {}）",
                Dialect::PodApp.profile_const(),
                Dialect::MiniApp.profile_const()
            )
        })?;

        let root = dialect.root_key();
        let ident_v = v
            .get(root)
            .ok_or_else(|| format!("{} 方言的清单缺少顶层 \"{root}\" 段", dialect.label()))?;
        let ident: PodIdent = serde_json::from_value(ident_v.clone())
            .map_err(|e| format!("\"{root}\" 段解析失败: {e}"))?;

        let package: Package =
            serde_json::from_value(v.get("package").cloned().ok_or("清单缺少 package 段")?)
                .map_err(|e| format!("package 段解析失败: {e}"))?;

        let ui: Ui = serde_json::from_value(v.get("ui").cloned().ok_or("清单缺少 ui 段")?)
            .map_err(|e| format!("ui 段解析失败: {e}"))?;

        let permissions: Perms = match v.get("permissions") {
            Some(p) => serde_json::from_value(p.clone())
                .map_err(|e| format!("permissions 段解析失败: {e}"))?,
            None => Perms::default(),
        };

        let mut extra = serde_json::Map::new();
        if let Some(o) = v.as_object() {
            for (k, val) in o {
                if k != root && !KNOWN_KEYS.contains(&k.as_str()) {
                    extra.insert(k.clone(), val.clone());
                }
            }
        }

        Ok(Self {
            dialect,
            ident,
            action_parity: v
                .get("action_parity")
                .and_then(|x| x.as_str())
                .map(String::from),
            package,
            ui,
            permissions,
            extra,
        })
    }

    /// 写成指定方言的清单 JSON。
    ///
    /// 与 [`Manifest::from_json`] 构成双向转换 —— `tests/roundtrip.rs` 靠这一对
    /// 断言两份标准语义等价。改了任一边都要让那条测试继续绿，否则就是开始分家了。
    pub fn to_json(&self, d: Dialect) -> Value {
        let mut o = serde_json::Map::new();
        o.insert("profile".into(), json!(d.profile_const()));
        o.insert(
            d.root_key().into(),
            serde_json::to_value(&self.ident).unwrap_or(Value::Null),
        );
        if let Some(ap) = &self.action_parity {
            o.insert("action_parity".into(), json!(ap));
        }
        o.insert(
            "package".into(),
            serde_json::to_value(&self.package).unwrap_or(Value::Null),
        );
        o.insert(
            "ui".into(),
            serde_json::to_value(&self.ui).unwrap_or(Value::Null),
        );
        o.insert(
            "permissions".into(),
            serde_json::to_value(&self.permissions).unwrap_or(Value::Null),
        );
        for (k, v) in &self.extra {
            o.insert(k.clone(), v.clone());
        }
        Value::Object(o)
    }

    /// 换一种方言的同一份清单。
    pub fn translated(&self, d: Dialect) -> Self {
        Self {
            dialect: d,
            ..self.clone()
        }
    }
}

// ───────────────────────────── 命名校验 ─────────────────────────────

pub fn valid_pod_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && !id.starts_with('.')
}

fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=24).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// 动作 ID 形如 `app.<slug>.<域>.<动词>`。
///
/// 前缀刻意保持 `app.` 而**不是** `pod.`：动作 ID 是写进 `action-parity.json` 的，
/// 那份文件两种方言共用，也要过上游官方校验器。为了品牌改它，代价是每个已有程序舱
/// 的动作 ID 全部失效 —— 而 ID 是外部契约，不是内部命名。
fn valid_action_id(id: &str, slug: &str) -> bool {
    let Some(rest) = id.strip_prefix(&format!("app.{slug}.")) else {
        return false;
    };
    let segs: Vec<&str> = rest.split('.').filter(|s| !s.is_empty()).collect();
    segs.len() >= 2
        && segs.iter().all(|s| {
            let b = s.as_bytes();
            b[0].is_ascii_lowercase()
                && b.iter().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-'
                })
        })
}

/// 这份 ActionParity 规范版本能不能吃。
///
/// 不维护一张精确列表 —— ActionParity 一天之内从 0.1.0 走到 0.5.0，每次都要改宿主追一遍，
/// 等于**上游一发版，用户已装的程序舱全废**，而第三方作者根本不知道是谁弄坏的。
///
/// 判断依据换成实际的兼容语义：迄今每一次变更都是**加字段**（顶层必填从没动过），
/// 而消费方只读自己认识的字段，多出来的字段天然无害。真正会破坏的是删字段/改必填 ——
/// 那种事该走大版本。所以：0.x 一律收，1.0 之后再谈。
fn spec_version_ok(v: &str) -> bool {
    let mut it = v.split('.');
    matches!(
        (it.next().and_then(|s| s.parse::<u32>().ok()), it.next()),
        (Some(0), Some(_))
    )
}

// ───────────────────────────── 装载与校验 ─────────────────────────────

/// 读一个已解包的目录，做完整校验。返回归一化清单和它的 `action-parity.json`。
pub fn load_dir(dir: &Path) -> Result<(Manifest, Value), String> {
    let dialect = Dialect::detect(dir)?;
    let mpath = dir.join(dialect.manifest_file());
    let text = std::fs::read_to_string(&mpath)
        .map_err(|e| format!("读不到 {}: {e}", dialect.manifest_file()))?;
    let raw: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} 解析失败: {e}", dialect.manifest_file()))?;
    let m = Manifest::from_json(&raw)?;

    if m.dialect != dialect {
        return Err(format!(
            "文件名是 {} 但 profile 写的是 {} —— 两者必须一致",
            dialect.manifest_file(),
            m.dialect.profile_const()
        ));
    }
    if !valid_pod_id(&m.ident.id) {
        return Err(format!("非法 id: {}", m.ident.id));
    }
    if !valid_slug(&m.ident.slug) {
        return Err(format!(
            "非法 slug: {}（只允许小写字母/数字/连字符，2-24 字符，禁下划线）",
            m.ident.slug
        ));
    }

    let rel = m
        .action_parity
        .clone()
        .unwrap_or_else(|| "./action-parity.json".into());
    let ppath =
        crate::safe_join(dir, rel.trim_start_matches("./")).ok_or("action_parity 路径非法")?;
    let ptext = std::fs::read_to_string(&ppath).map_err(|e| format!("读不到 {rel}: {e}"))?;
    let parity: Value = serde_json::from_str(&ptext).map_err(|e| format!("{rel} 解析失败: {e}"))?;

    let sv = parity
        .get("spec_version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !spec_version_ok(sv) {
        return Err(format!(
            "这个{}用的是 ActionParity {sv}，当前宿主只吃 0.x —— 升级宿主试试",
            dialect.label()
        ));
    }
    let pid = parity
        .pointer("/application/id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if pid != m.ident.id {
        return Err(format!(
            "身份不一致: {}={} action-parity.json={pid}",
            dialect.manifest_file(),
            m.ident.id
        ));
    }
    let pver = parity
        .pointer("/application/version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if pver != m.ident.version {
        return Err(format!(
            "版本不一致: {}={} action-parity.json={pver}",
            dialect.manifest_file(),
            m.ident.version
        ));
    }

    let acts = parity
        .get("actions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if acts.is_empty() {
        return Err("action-parity.json 里一个动作都没有".into());
    }
    let mut any_headless = false;
    for a in &acts {
        let id = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !valid_action_id(id, &m.ident.slug) {
            return Err(format!(
                "动作 \"{id}\" 不在命名空间 app.{}. 内",
                m.ident.slug
            ));
        }
        if a.pointer("/execution/headless").and_then(|v| v.as_bool()) == Some(true) {
            any_headless = true;
        }
    }

    // 无头实现必须真实存在，否则 CLI/MCP/影核那三个面全是空头支票
    match m.package.kind.as_str() {
        "web" => {
            let w = m
                .package
                .web
                .clone()
                .ok_or("package.kind=web 但缺 package.web")?;
            let root = crate::safe_join(dir, &w.root).ok_or("package.web.root 路径非法")?;
            if !crate::safe_join(&root, &w.entry)
                .map(|p| p.exists())
                .unwrap_or(false)
            {
                return Err(format!("入口文件不存在: {}/{}", w.root, w.entry));
            }
            if any_headless {
                let am = w.actions.clone().ok_or(
                    "有动作声明 headless=true，但 package.web.actions 没填 —— 无头调用时没有实现可跑",
                )?;
                if !crate::safe_join(&root, &am)
                    .map(|p| p.exists())
                    .unwrap_or(false)
                {
                    return Err(format!("动作模块不存在: {}/{am}", w.root));
                }
            }
        }
        "script" => {
            let s = m
                .package
                .script
                .clone()
                .ok_or("package.kind=script 但缺 package.script")?;
            if !crate::safe_join(dir, &s.skill_dir)
                .map(|p| p.exists())
                .unwrap_or(false)
            {
                return Err(format!("skill 目录不存在: {}", s.skill_dir));
            }
        }
        "native" => {
            let n = m
                .package
                .native
                .clone()
                .ok_or("package.kind=native 但缺 package.native")?;
            if !crate::safe_join(dir, &n.exe)
                .map(|p| p.exists())
                .unwrap_or(false)
            {
                return Err(format!("可执行文件不存在: {}", n.exe));
            }
        }
        other => return Err(format!("不认识的 package.kind: {other}")),
    }

    if !m.ui.icon.starts_with("lucide:")
        && !crate::safe_join(dir, &m.ui.icon)
            .map(|p| p.exists())
            .unwrap_or(false)
    {
        return Err(format!("图标文件不存在: {}", m.ui.icon));
    }

    // 引用完整性：界面上点得到的东西必须真的存在
    let ids: Vec<&str> = acts
        .iter()
        .filter_map(|a| a.get("id").and_then(|v| v.as_str()))
        .collect();
    for q in &m.ui.quick_actions {
        if !ids.contains(&q.action.as_str()) {
            return Err(format!("ui.quick_actions 引用了不存在的动作 {}", q.action));
        }
    }
    if let Some(an) = &m.ui.annotation {
        if !ids.contains(&an.action.as_str()) {
            return Err(format!("ui.annotation 引用了不存在的动作 {}", an.action));
        }
    }
    for h in &m.permissions.host_actions {
        if h.starts_with("app.") {
            return Err(format!(
                "permissions.host_actions 里的 {h} 是{}动作，这里只填宿主动作",
                dialect.label()
            ));
        }
    }

    if let Some(min) = &m.ident.min_host_version {
        let host = &crate::profile().host_version;
        if crate::version_lt(host, min) {
            return Err(format!(
                "这个{}需要宿主 {min} 或更新，当前是 {host}",
                dialect.label()
            ));
        }
    }

    Ok((m, parity))
}

// ───────────────────────────── 查询 ─────────────────────────────

/// 这个目录是不是开发态（脚手架产物放在 `.dev/` 下，不缓存、不占首页）。
///
/// 比对的是**整个目录名**而不是字符串后缀 —— `Path::ends_with` 按路径分量匹配，
/// 写成那样容易被读成「以 .dev 结尾的扩展名」，所以这里写明白。
pub(crate) fn is_dev_dir(dir: &Path) -> bool {
    dir.parent()
        .and_then(|p| p.file_name())
        .map(|n| n == ".dev")
        .unwrap_or(false)
}

/// 正式安装目录优先，其次开发态目录。
pub(crate) fn resolve_dir(id: &str) -> Option<PathBuf> {
    if !valid_pod_id(id) {
        return None;
    }
    let has_manifest = |d: &Path| {
        Dialect::all()
            .iter()
            .any(|dl| d.join(dl.manifest_file()).exists())
    };
    let a = crate::apps_root().join(id);
    if has_manifest(&a) {
        return Some(a);
    }
    let d = crate::apps_root().join(".dev").join(id);
    has_manifest(&d).then_some(d)
}

pub fn info_of(
    dir: &Path,
    pinned: Option<bool>,
    enabled: Option<bool>,
    dev: bool,
) -> Option<PodInfo> {
    let (m, parity) = load_dir(dir).ok()?;
    let actions = parity
        .get("actions")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some(PodInfo {
        id: m.ident.id.clone(),
        slug: m.ident.slug.clone(),
        name: m.ident.name.clone(),
        version: m.ident.version.clone(),
        summary: m.ident.summary.clone(),
        kind: m.package.kind.clone(),
        icon: m.ui.icon.clone(),
        accent: m.ui.accent.clone(),
        container: m.ui.container.clone(),
        home_dock: m.ui.home_dock,
        pinned_home: pinned.unwrap_or(m.ui.home_dock),
        enabled: enabled.unwrap_or(true),
        actions,
        quick_actions: m
            .ui
            .quick_actions
            .iter()
            .map(|q| json!({ "action": q.action, "label": q.label, "icon": q.icon }))
            .collect(),
        permissions: m.permissions.summary(),
        dev,
    })
}

pub fn get(id: &str) -> Result<PodInfo, String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个程序舱: {id}"))?;
    let dev = is_dev_dir(&dir);
    let reg = crate::registry::read();
    let e = reg.apps.iter().find(|e| e.id == id);
    info_of(&dir, e.map(|e| e.pinned_home), e.map(|e| e.enabled), dev)
        .ok_or_else(|| format!("清单读不出来: {id}"))
}

/// 这个程序舱的 `action-parity.json`。
pub fn parity_of(id: &str) -> Result<Value, String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个程序舱: {id}"))?;
    Ok(load_dir(&dir)?.1)
}

pub fn permissions(id: &str) -> Result<Perms, String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个程序舱: {id}"))?;
    Ok(load_dir(&dir)?.0.permissions)
}

/// 由动作 ID 反查它属于哪个程序舱。
pub fn owner_of(action_id: &str) -> Option<String> {
    let slug = action_id.strip_prefix("app.")?.split('.').next()?;
    crate::registry::list()
        .into_iter()
        .find(|i| i.slug == slug)
        .map(|i| i.id)
}

/// 把所有已装程序舱的动作摊平成 [`crate::ActionSpec`]，并进宿主动作总线。
pub fn action_specs() -> Vec<crate::ActionSpec> {
    let mut out = vec![];
    for i in crate::registry::list() {
        if !i.enabled {
            continue;
        }
        let Ok(parity) = parity_of(&i.id) else {
            continue;
        };
        let Some(acts) = parity.get("actions").and_then(|v| v.as_array()) else {
            continue;
        };
        out.extend(acts.iter().filter_map(crate::ActionSpec::from_parity));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_id_namespace_is_enforced() {
        assert!(valid_action_id("app.ninegrid.image.split", "ninegrid"));
        assert!(valid_action_id("app.qr.code.replace", "qr"));
        // 别人的命名空间、段数不够、大写、缺前缀 —— 全拒
        assert!(!valid_action_id("app.other.image.split", "ninegrid"));
        assert!(!valid_action_id("app.ninegrid.split", "ninegrid"));
        assert!(!valid_action_id("app.ninegrid.Image.split", "ninegrid"));
        assert!(!valid_action_id("ninegrid.image.split", "ninegrid"));
    }

    #[test]
    fn slug_and_id_rules() {
        assert!(valid_slug("nine-grid"));
        assert!(!valid_slug("a"), "太短");
        assert!(!valid_slug("Nine"), "大写");
        assert!(!valid_slug("nine_grid"), "下划线");
        assert!(valid_pod_id("org.podapp.image.nine-grid"));
        assert!(!valid_pod_id(".hidden"), "点开头会撞上内部目录");
        assert!(!valid_pod_id("has/slash"));
    }

    #[test]
    fn spec_version_accepts_all_of_0x() {
        for v in ["0.1.0", "0.3.0", "0.5.0", "0.99.1"] {
            assert!(
                spec_version_ok(v),
                "{v} 该被接受 —— 上游加字段不能让老包失效"
            );
        }
        for v in ["1.0.0", "", "abc"] {
            assert!(!spec_version_ok(v));
        }
    }
}
