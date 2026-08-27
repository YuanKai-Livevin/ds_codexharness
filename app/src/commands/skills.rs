//! SKILLS 技能库命令：扫描、打开、导入。

use crate::app_state::data_root;

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillInfo {
    name: String,
    description: String,
    path: String,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillsRepoInfo {
    path: String,
    skills: Vec<SkillInfo>,
}

/// 读取 SKILLS 仓库（固定工作区）路径与技能列表（扫描 SKILL.md）。
#[tauri::command]
pub(crate) async fn get_skills_repo() -> Result<SkillsRepoInfo, String> {
    let repo = data_root().join("skills");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&repo) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let md = dir.join("SKILL.md");
            if md.exists() {
                if let Ok(content) = std::fs::read_to_string(&md) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let description = parse_skill_description(&content);
                    skills.push(SkillInfo {
                        name,
                        description,
                        path: dir.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(SkillsRepoInfo {
        path: repo.to_string_lossy().to_string(),
        skills,
    })
}

/// 解析 SKILL.md 的 frontmatter description。
fn parse_skill_description(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return String::new();
    }
    let end = trimmed[3..].find("---").map(|i| i + 3).unwrap_or(trimmed.len());
    let front = &trimmed[3..end];
    for line in front.lines() {
        if let Some(v) = line.strip_prefix("description:") {
            return v.trim().trim_matches('"').trim_matches('\'').to_string();
        }
    }
    String::new()
}

/// 在资源管理器中打开 SKILLS 仓库。
#[tauri::command]
pub(crate) async fn open_skills_repo() -> Result<(), String> {
    let repo = data_root().join("skills");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&repo)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 从所选目录导入技能（含 SKILL.md 的文件夹复制到技能仓库），返回导入的技能名。
#[tauri::command]
pub(crate) async fn import_skills(dir: String) -> Result<Vec<String>, String> {
    let repo = data_root().join("skills");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    let src = std::path::Path::new(&dir);
    if !src.is_dir() {
        return Err("所选路径不是文件夹。".to_string());
    }
    let mut candidates: Vec<std::path::PathBuf> = vec![src.to_path_buf()];
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
        let Some(name) = c.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        let dst = repo.join(&name);
        if dst.exists() {
            std::fs::remove_dir_all(&dst).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&c, &dst).map_err(|e| format!("复制 {} 失败: {}", name, e))?;
        imported.push(name);
    }
    Ok(imported)
}

pub(crate) fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
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
