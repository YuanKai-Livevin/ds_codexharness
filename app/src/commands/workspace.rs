//! 工作区 Tauri 命令：切换/移除、浏览、@引用、打开。

use crate::app_state::{data_root, AppState};
use oh_core::workspace;
use serde::Serialize;
use tauri::State;

#[derive(Serialize, Clone)]
pub(crate) struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: String,
}

/// 常用位置（桌面/文档/下载），供工作区快捷切换。
#[tauri::command]
pub(crate) async fn common_folders() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(d) = dirs::desktop_dir() {
        out.push(("桌面".to_string(), d.to_string_lossy().to_string()));
    }
    if let Some(d) = dirs::document_dir() {
        out.push(("文档".to_string(), d.to_string_lossy().to_string()));
    }
    if let Some(d) = dirs::download_dir() {
        out.push(("下载".to_string(), d.to_string_lossy().to_string()));
    }
    out
}

/// 从工作区列表移除一个工作区。
/// 若删除的是当前工作区，自动切换到下一个可用工作区（或默认），返回新工作区路径。
/// SKILLS 仓库为固定工作区，不可删除。
#[tauri::command]
pub(crate) async fn remove_workspace(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let skills_repo = data_root().join("skills").to_string_lossy().to_string();
    if path == skills_repo {
        return Err("SKILLS 仓库是固定工作区，不可删除。".to_string());
    }
    let mut settings = state.settings.lock().await.clone();
    settings.recent_workspaces.retain(|w| w != &path);
    let mut switched = false;
    if settings.workspace_path == path {
        let next = settings.recent_workspaces.first().cloned();
        let fallback = dirs::document_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("办公工作区")
            .to_string_lossy()
            .to_string();
        settings.workspace_path = next.unwrap_or(fallback);
        settings.recent_workspaces.retain(|w| w != &settings.workspace_path);
        switched = true;
    }
    settings.save(&state.settings_path)?;
    let new_ws = settings.workspace_path.clone();
    *state.settings.lock().await = settings;
    Ok(if switched { new_ws } else { String::new() })
}

/// 在资源管理器中打开当前工作区。
#[tauri::command]
pub(crate) async fn open_workspace(state: State<'_, AppState>) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    if !ws.is_dir() {
        return Err("工作区路径不是文件夹，请到「设置」中重新选择。".to_string());
    }
    std::process::Command::new("explorer")
        .arg(&ws)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 原生文件夹选择（rfd：Rust 原生 IFileDialog，仅允许选择文件夹）。
#[tauri::command]
pub(crate) async fn pick_folder() -> Result<Option<String>, String> {
    let handle = tauri::async_runtime::spawn(async move {
        rfd::AsyncFileDialog::new()
            .set_title("选择工作区文件夹（请点底部「选择文件夹」按钮）")
            .pick_folder()
            .await
    });
    let folder = handle
        .await
        .map_err(|e| format!("文件夹选择器异常: {}", e))?;
    match folder {
        Some(f) => {
            let p = f.path().to_string_lossy().to_string();
            // 二次校验：必须是文件夹
            if std::path::Path::new(&p).is_file() {
                Err("所选路径是一个文件，请重新选择一个文件夹。".to_string())
            } else {
                Ok(Some(p))
            }
        }
        None => Ok(None),
    }
}

/// 列出工作区内目录（严格限制在工作区范围）。
#[tauri::command]
pub(crate) async fn list_dir(state: State<'_, AppState>, path: String) -> Result<Vec<FileEntry>, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许浏览工作区目录内的文件。".to_string());
    }
    if !target.is_dir() {
        return Err("指定路径不是目录。".to_string());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(target).map_err(|e| e.to_string())? {
        let Ok(e) = entry else { continue };
        let md = e.metadata().ok();
        out.push(FileEntry {
            name: e.file_name().to_string_lossy().to_string(),
            is_dir: md.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            size: md.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: md
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
                .unwrap_or(0)
                .to_string(),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// 递归列出工作区内全部文件（相对路径），供 @ 引用选择。限制数量避免超大工作区卡顿。
#[tauri::command]
pub(crate) async fn list_workspace_files(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let mut out = Vec::new();
    let mut stack = vec![ws.clone()];
    let max = 800usize;
    while let Some(dir) = stack.pop() {
        if out.len() >= max {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if out.len() >= max {
                break;
            }
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if md.is_dir() {
                // 跳过隐藏目录与临时目录
                if name.starts_with('.') || name == "node_modules" || name == ".oh_tmp" {
                    continue;
                }
                stack.push(entry.path());
            } else if md.is_file() {
                if name.starts_with('.') {
                    continue;
                }
                let rel = entry
                    .path()
                    .strip_prefix(&ws)
                    .unwrap_or(entry.path().as_path())
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 在工作区内的路径上执行系统操作：打开（默认程序）或"在文件夹中显示"。
#[tauri::command]
pub(crate) async fn open_path(state: State<'_, AppState>, path: String, reveal: bool) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许操作工作区目录内的文件。".to_string());
    }
    if !target.exists() {
        return Err("文件不存在：".to_string() + &path);
    }
    if reveal {
        // 在文件夹中显示（高亮该文件）
        let mut cmd = std::process::Command::new("explorer");
        cmd.arg("/select,").arg(&target);
        cmd.spawn().map_err(|e| e.to_string())?;
    } else if target.is_dir() {
        // 文件夹：explorer 直接打开
        std::process::Command::new("explorer")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    } else {
        // 文件：用系统默认程序打开（Start-Process，避免 explorer 弹「选择打开方式」）
        let quoted = target.to_string_lossy().replace('\'', "''");
        let mut cmd = std::process::Command::new("powershell");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!("Start-Process -FilePath '{}'", quoted),
        ])
        .spawn()
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
