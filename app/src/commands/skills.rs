//! SKILLS 技能治理（R8 / T1-04）：元数据、启用/禁用、导入预览+备份、测试任务、回滚。

use crate::app_state::data_root;
use oh_core::python::Bundled;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillMeta {
    name: String,
    version: String,
    description: String,
    author: String,
    permissions: String,
    checksum: String,
    disabled: bool,
    has_selftest: bool,
    has_backup: bool,
    file_count: usize,
    path: String,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillsRepoInfo {
    path: String,
    skills: Vec<SkillMeta>,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct ImportedSkill {
    name: String,
    version: String,
    description: String,
    permissions: String,
    checksum: String,
    replaced: bool,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillTestResult {
    ran: bool,
    exit_code: Option<i32>,
    output_tail: String,
}

struct SkillFront {
    name: String,
    version: String,
    description: String,
    author: String,
    permissions: String,
}

impl Default for SkillFront {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "1.0.0".into(),
            description: String::new(),
            author: "未知".into(),
            permissions: "工作区内读写".into(),
        }
    }
}

/// 解析 SKILL.md 的 YAML frontmatter（name/version/description/author/permissions）。
fn parse_frontmatter(content: &str) -> SkillFront {
    let mut f = SkillFront::default();
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        let end = trimmed[3..].find("---").map(|i| i + 3).unwrap_or(trimmed.len());
        let front = &trimmed[3..end];
        for line in front.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let k = k.trim();
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            match k {
                "name" => f.name = v,
                "version" => f.version = v,
                "description" => f.description = v,
                "author" => f.author = v,
                "permissions" => f.permissions = v,
                _ => {}
            }
        }
    }
    f
}

/// FNV-1a 64 校验和（SKILL.md 内容完整性展示）。
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", h)
}

fn file_count(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                n += file_count(&e.path());
            } else {
                n += 1;
            }
        }
    }
    n
}

fn skill_dir() -> PathBuf {
    let repo = data_root().join("skills");
    let _ = std::fs::create_dir_all(&repo);
    repo
}

fn disabled_dir() -> PathBuf {
    let d = data_root().join("disabled-skills");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn backup_root() -> PathBuf {
    let d = data_root().join("skill-backups");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn has_selftest(dir: &Path) -> bool {
    dir.join("selftest.py").exists() || dir.join("otools.py").exists()
}

fn scan_skill(dir: &Path, name: &str, disabled: bool) -> Option<SkillMeta> {
    let md = dir.join("SKILL.md");
    if !md.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&md).ok()?;
    let front = parse_frontmatter(&content);
    let name = if front.name.is_empty() { name.to_string() } else { front.name.clone() };
    let checksum = fnv1a_hex(content.as_bytes());
    let backups = backup_root().join(&name);
    let has_backup = std::fs::read_dir(&backups).map(|mut it| it.next().is_some()).unwrap_or(false);
    Some(SkillMeta {
        name,
        version: front.version,
        description: front.description,
        author: front.author,
        permissions: front.permissions,
        checksum,
        disabled,
        has_selftest: has_selftest(dir),
        has_backup,
        file_count: file_count(dir),
        path: dir.to_string_lossy().to_string(),
    })
}

/// 读取 SKILLS 仓库：启用技能（skills/）与禁用技能（disabled-skills/）统一列出。
#[tauri::command]
pub(crate) async fn get_skills_repo() -> Result<SkillsRepoInfo, String> {
    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(skill_dir()) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if dir.is_dir() {
                if let Some(m) = scan_skill(&dir, &entry.file_name().to_string_lossy(), false) {
                    skills.push(m);
                }
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(disabled_dir()) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if dir.is_dir() {
                if let Some(m) = scan_skill(&dir, &entry.file_name().to_string_lossy(), true) {
                    skills.push(m);
                }
            }
        }
    }
    skills.sort_by(|a, b| (a.disabled, &a.name).cmp(&(b.disabled, &b.name)));
    Ok(SkillsRepoInfo {
        path: skill_dir().to_string_lossy().to_string(),
        skills,
    })
}

/// 在资源管理器中打开 SKILLS 仓库。
#[tauri::command]
pub(crate) async fn open_skills_repo() -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(skill_dir())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 导入技能：复制到仓库；同名已存在时先备份再替换；返回元数据（导入预览信息）。
#[tauri::command]
pub(crate) async fn import_skills(dir: String) -> Result<Vec<ImportedSkill>, String> {
    let repo = skill_dir();
    let src = std::path::Path::new(&dir);
    if !src.is_dir() {
        return Err("所选路径不是文件夹。".to_string());
    }
    let mut candidates: Vec<PathBuf> = vec![src.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                candidates.push(entry.path());
            }
        }
    }
    let mut imported = Vec::new();
    for c in candidates {
        let md = c.join("SKILL.md");
        if !md.exists() {
            continue;
        }
        let Some(dir_name) = c.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        let content = std::fs::read_to_string(&md).map_err(|e| e.to_string())?;
        let front = parse_frontmatter(&content);
        let name = if front.name.is_empty() { dir_name.clone() } else { front.name.clone() };
        let dst = repo.join(&dir_name);
        let mut replaced = false;
        if dst.exists() {
            // 回滚备份：.skill-backups/<name>/<ts>/
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let bk = backup_root().join(&name).join(ts.to_string());
            std::fs::create_dir_all(&bk).map_err(|e| e.to_string())?;
            copy_dir_recursive(&dst, &bk).map_err(|e| format!("备份旧版失败: {}", e))?;
            std::fs::remove_dir_all(&dst).map_err(|e| e.to_string())?;
            replaced = true;
        }
        copy_dir_recursive(&c, &dst).map_err(|e| format!("复制 {} 失败: {}", name, e))?;
        imported.push(ImportedSkill {
            name,
            version: front.version,
            description: front.description,
            permissions: front.permissions,
            checksum: fnv1a_hex(content.as_bytes()),
            replaced,
        });
    }
    Ok(imported)
}

/// 禁用技能：移出技能仓库（模型将不再看到），可随时恢复。
#[tauri::command]
pub(crate) async fn disable_skill(name: String) -> Result<(), String> {
    let from = skill_dir().join(&name);
    if !from.join("SKILL.md").exists() {
        return Err(format!("技能不存在: {}", name));
    }
    let to = disabled_dir().join(&name);
    if to.exists() {
        std::fs::remove_dir_all(&to).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&from, &to).map_err(|e| format!("禁用失败: {}", e))?;
    Ok(())
}

/// 启用技能：移回技能仓库。
#[tauri::command]
pub(crate) async fn enable_skill(name: String) -> Result<(), String> {
    let from = disabled_dir().join(&name);
    if !from.join("SKILL.md").exists() {
        return Err(format!("已禁用的技能不存在: {}", name));
    }
    let to = skill_dir().join(&name);
    if to.exists() {
        return Err(format!("技能仓库中已存在同名技能: {}", name));
    }
    std::fs::rename(&from, &to).map_err(|e| format!("启用失败: {}", e))?;
    Ok(())
}

/// 测试技能：运行技能内置测试（selftest.py 或 otools.py selftest）。
#[tauri::command]
pub(crate) async fn test_skill(app: AppHandle, name: String) -> Result<SkillTestResult, String> {
    let mut dir = skill_dir().join(&name);
    if !dir.join("SKILL.md").exists() {
        dir = disabled_dir().join(&name);
    }
    if !dir.join("SKILL.md").exists() {
        return Err(format!("技能不存在: {}", name));
    }
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let py = bundled.python_exe();
    let (script, args) = if dir.join("selftest.py").exists() {
        (dir.join("selftest.py"), Vec::<String>::new())
    } else if dir.join("otools.py").exists() {
        (
            dir.join("otools.py"),
            vec!["selftest".to_string(), "--root".to_string(), dir.to_string_lossy().to_string()],
        )
    } else {
        return Ok(SkillTestResult {
            ran: false,
            exit_code: None,
            output_tail: "该技能没有内置测试脚本（selftest.py）。".into(),
        });
    };
    let out = std::process::Command::new(&py)
        .arg(&script)
        .args(&args)
        .current_dir(&dir)
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .map_err(|e| format!("运行测试失败: {}", e))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    let lines: Vec<&str> = text.lines().collect();
    let tail: Vec<&str> = lines.iter().rev().take(40).copied().collect();
    let tail = tail.iter().rev().cloned().collect::<Vec<_>>().join("\n");
    Ok(SkillTestResult {
        ran: true,
        exit_code: out.status.code(),
        output_tail: tail,
    })
}

/// 回滚技能：恢复到最近一次导入前的备份版本。
#[tauri::command]
pub(crate) async fn rollback_skill(name: String) -> Result<(), String> {
    let root = backup_root().join(&name);
    let mut backups: Vec<PathBuf> = std::fs::read_dir(&root)
        .map_err(|e| format!("没有该技能的备份: {}", e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    backups.sort();
    let latest = backups.pop().ok_or_else(|| "没有可回滚的备份版本。".to_string())?;
    // 备份文件名为秒级时间戳，可能存在多个；取最新（文件名最大）
    let dst = skill_dir().join(&name);
    if dst.join("SKILL.md").exists() {
        std::fs::remove_dir_all(&dst).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&latest, &dst).map_err(|e| format!("回滚失败: {}", e))?;
    Ok(())
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
