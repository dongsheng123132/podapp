//! 安装 / 卸载 —— staging → 校验 → 原子换入。
//!
//! 顺序不能反。先解包到 staging、在那里跑完整校验、**只有全过了**才 rename 进正式目录。
//! 直接往目标目录解包再校验的话，一个坏包会把用户已经装好的那份毁掉，
//! 而错误信息只会说「校验失败」—— 用户失去的东西和错误信息完全对不上。
//!
//! 加固不可省：手写归档处理是供应链 CVE 的老巢。条目数、单文件大小、总量、
//! 符号链接、路径穿越、扩展名白名单，六道全都要在。

use crate::dialect::Dialect;
use crate::manifest::{load_dir, resolve_dir, PodInfo};
use crate::registry::{self, RegEntry};
use serde_json::{json, Value};
use std::path::Path;

const MAX_ENTRIES: usize = 2000;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_ENTRY: u64 = 8 * 1024 * 1024;

/// 清单文件名一律放行（两种方言都算）。
fn is_manifest_name(name: &str) -> bool {
    name == "action-parity.json" || Dialect::all().iter().any(|d| d.manifest_file() == name)
}

/// 扩展名白名单。**默认拒绝**：包里出现没见过的类型时，拒绝比放行安全 ——
/// 放行一个 `.bat` 的代价远大于让作者多问一句「为什么我的文件被拒了」。
fn ext_allowed(kind: &str, name: &str) -> bool {
    let e = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let base: &[&str] = &[
        "html", "htm", "js", "mjs", "css", "json", "png", "jpg", "jpeg", "webp", "gif", "svg",
        "woff2", "ttf", "md", "txt",
    ];
    if base.contains(&e.as_str()) {
        return true;
    }
    match kind {
        "script" => ["py", "sh", "mjs", "cjs"].contains(&e.as_str()),
        "native" => ["exe", "dll", "so", "dylib"].contains(&e.as_str()),
        _ => false,
    }
}

/// 解包 tar.gz（`.pod` / `.ukapp` 都是这个格式）到 staging。
fn extract_targz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut ar = tar::Archive::new(gz);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;

    let mut n = 0usize;
    let mut total = 0u64;
    for entry in ar.entries().map_err(|e| format!("归档读取失败: {e}"))? {
        let mut e = entry.map_err(|e| format!("归档条目损坏: {e}"))?;
        n += 1;
        if n > MAX_ENTRIES {
            return Err(format!("条目过多（上限 {MAX_ENTRIES}）"));
        }
        let kind = e.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err("包里含符号/硬链接，拒绝安装".into());
        }
        let path = e
            .path()
            .map_err(|_| "条目路径非法".to_string())?
            .to_path_buf();
        let rel = path.to_string_lossy().replace('\\', "/");
        if rel.starts_with('/') || rel.contains("..") || rel.contains(':') {
            return Err(format!("条目路径越界: {rel}"));
        }
        let Some(out) = crate::safe_join(dest, &rel) else {
            return Err(format!("条目路径非法: {rel}"));
        };
        if kind.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let size = e.header().size().unwrap_or(0);
        if size > MAX_ENTRY {
            return Err(format!("单个文件过大: {rel}"));
        }
        total += size;
        if total > MAX_TOTAL {
            return Err(format!(
                "解压总量超限（上限 {}MB）",
                MAX_TOTAL / 1024 / 1024
            ));
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::with_capacity(size as usize);
        e.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, kind: &str, budget: &mut (usize, u64)) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            return Err(format!("拒绝符号链接: {name}"));
        }
        let src = e.path();
        let dst = to.join(&name);
        if ft.is_dir() {
            copy_tree(&src, &dst, kind, budget)?;
        } else {
            budget.0 += 1;
            if budget.0 > MAX_ENTRIES {
                return Err("文件过多".into());
            }
            let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
            if sz > MAX_ENTRY {
                return Err(format!("文件过大: {name}"));
            }
            budget.1 += sz;
            if budget.1 > MAX_TOTAL {
                return Err("总量超限".into());
            }
            if !ext_allowed(kind, &name) && !is_manifest_name(&name) {
                return Err(format!("不允许的文件类型: {name}"));
            }
            std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 从目录或 `.pod` / `.ukapp` 包安装。校验通过后原子换入（同卷 rename）。
pub fn install_from_path(src: &Path, source_label: &str) -> Result<PodInfo, String> {
    let root = crate::apps_root();
    std::fs::create_dir_all(crate::staging_root()).map_err(|e| e.to_string())?;
    let stage = crate::staging_root().join(format!("s{}", crate::now_ms()));
    let _ = std::fs::remove_dir_all(&stage);

    if src.is_dir() {
        // 先读一遍清单拿 kind（扩展名白名单要按 kind 放宽）
        let dialect = Dialect::detect(src)?;
        let probe = std::fs::read_to_string(src.join(dialect.manifest_file()))
            .map_err(|e| format!("读不到 {}: {e}", dialect.manifest_file()))?;
        let kind = serde_json::from_str::<Value>(&probe)
            .ok()
            .and_then(|v| {
                v.pointer("/package/kind")
                    .and_then(|k| k.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| "web".into());
        let mut budget = (0usize, 0u64);
        copy_tree(src, &stage, &kind, &mut budget)?;
    } else {
        let bytes = std::fs::read(src).map_err(|e| format!("读不到包文件: {e}"))?;
        // zip 的魔数是 PK。给一句人能照做的话，别只说「格式不对」。
        if bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4b {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(format!(
                "这是 zip 包。当前只支持 tar.gz 格式的 .{}，请用 `podapp pack` 重新打包",
                crate::profile().dialect.pkg_ext()
            ));
        }
        if let Err(e) = extract_targz(&bytes, &stage) {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
    }

    // 校验在 staging 里做完，坏包碰不到用户已装的那份
    let (m, _) = match load_dir(&stage) {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
    };

    let target = root.join(&m.ident.id);
    if target.exists() {
        std::fs::create_dir_all(crate::trash_root()).map_err(|e| e.to_string())?;
        let bak = crate::trash_root().join(format!("{}-{}", m.ident.id, crate::now_ms()));
        std::fs::rename(&target, &bak).map_err(|e| format!("旧版本挪不开: {e}"))?;
    }
    std::fs::rename(&stage, &target).map_err(|e| format!("换入失败: {e}"))?;

    let _ = std::fs::write(
        target.join(".install.json"),
        json!({
            "source": source_label,
            "installed_at": crate::now_ms(),
            "version": m.ident.version,
            "dialect": m.dialect.profile_const(),
        })
        .to_string(),
    );

    let mut reg = registry::read();
    reg.version = 1;
    reg.apps.retain(|a| a.id != m.ident.id);
    reg.apps.push(RegEntry {
        id: m.ident.id.clone(),
        enabled: true,
        pinned_home: m.ui.home_dock,
        installed_at: crate::now_ms(),
        source: source_label.to_string(),
    });
    registry::write(&reg);
    purge_trash();

    crate::manifest::get(&m.ident.id)
}

/// 回收站里超过一周的备份清掉。**不是立刻删** —— 升级出问题时那份旧目录是唯一的退路。
fn purge_trash() {
    let week = 7 * 24 * 3600 * 1000i64;
    let now = crate::now_ms();
    if let Ok(rd) = std::fs::read_dir(crate::trash_root()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(ts) = name.rsplit('-').next().and_then(|s| s.parse::<i64>().ok()) {
                if now - ts > week {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// 卸载。默认**留着**用户数据 —— 卸载重装是常见操作，顺手删数据不是用户的意思。
pub fn uninstall(id: &str, purge_data: bool) -> Result<(), String> {
    let dir = resolve_dir(id).ok_or_else(|| format!("没装这个程序舱: {id}"))?;
    std::fs::create_dir_all(crate::trash_root()).map_err(|e| e.to_string())?;
    let bak = crate::trash_root().join(format!("{id}-{}", crate::now_ms()));
    std::fs::rename(&dir, &bak).map_err(|e| format!("卸载失败: {e}"))?;
    if purge_data {
        let _ = std::fs::remove_dir_all(crate::data_dir(id));
    }
    let mut reg = registry::read();
    reg.apps.retain(|a| a.id != id);
    registry::write(&reg);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_manifest_names_are_always_allowed() {
        assert!(is_manifest_name("podapp.json"));
        assert!(is_manifest_name("uking-app.json"));
        assert!(is_manifest_name("action-parity.json"));
        assert!(!is_manifest_name("package.json"));
    }

    #[test]
    fn web_pods_cannot_ship_executables() {
        // 一个 web 程序舱夹带 .exe / .bat / .ps1 没有正当理由，必须拒
        for bad in ["evil.exe", "run.bat", "x.ps1", "a.dll", "s.sh", "t.py"] {
            assert!(!ext_allowed("web", bad), "{bad} 不该被 web 形态放行");
        }
        for ok in ["index.html", "actions.mjs", "icon.png", "style.css"] {
            assert!(ext_allowed("web", ok), "{ok} 该放行");
        }
        // native 形态才放行可执行文件，且仍不含脚本
        assert!(ext_allowed("native", "tool.exe"));
        assert!(!ext_allowed("native", "run.bat"));
    }
}
