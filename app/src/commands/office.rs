//! LibreOffice 命令：状态、打开、转换。

use crate::app_state::AppState;
use oh_core::python::Bundled;
use oh_core::workspace;
use tauri::{AppHandle, Manager, State};

/// LibreOffice 是否已内置。
#[tauri::command]
pub(crate) async fn libreoffice_status(app: AppHandle) -> Result<bool, String> {
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    Ok(bundled.libreoffice_available())
}

/// 用 LibreOffice 打开工作区内的文件。
#[tauri::command]
pub(crate) async fn open_in_libreoffice(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许操作工作区目录内的文件。".to_string());
    }
    if !target.exists() {
        return Err("文件不存在：".to_string() + &path);
    }
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let soffice = bundled.soffice_exe();
    if !soffice.exists() {
        return Err("未找到内置 LibreOffice（LibreOffice 目录缺失）。".to_string());
    }
    // 用 LibreOffice 打开（GUI）
    let mut cmd = std::process::Command::new(&soffice);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.arg("--norestore").arg(&target);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// 用 LibreOffice headless 将工作区文件转换为目标格式（pdf/docx/xlsx/odt/ods...）。
/// 输出到工作区下 .oh_convert 目录，返回输出文件绝对路径。
#[tauri::command]
pub(crate) async fn convert_office(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    to_format: String,
) -> Result<String, String> {
    let s = state.settings.lock().await.clone();
    let ws = workspace::ensure_workspace(&s.workspace_path)?;
    let target = std::path::Path::new(&path);
    if !workspace::is_within_workspace(&ws, target) {
        return Err("越界：仅允许操作工作区目录内的文件。".to_string());
    }
    if !target.exists() {
        return Err("文件不存在：".to_string() + &path);
    }
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let soffice = bundled.soffice_exe();
    if !soffice.exists() {
        return Err("未找到内置 LibreOffice。".to_string());
    }
    let fmt = to_format.trim().trim_start_matches('.').to_lowercase();
    if fmt.is_empty() {
        return Err("请指定目标格式（如 pdf / docx / xlsx / odt / ods）。".to_string());
    }
    let out_dir = ws.join(".oh_convert");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let out_dir_str = out_dir.to_string_lossy().to_string();
    let mut cmd = std::process::Command::new(&soffice);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.arg("--headless")
        .arg("--convert-to")
        .arg(&fmt)
        .arg("--outdir")
        .arg(&out_dir_str)
        .arg(&target);
    let out = cmd.output().map_err(|e| format!("启动 LibreOffice 失败: {}", e))?;
    // 输出文件：与源文件同名，扩展名改为目标格式
    let stem = target.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let result = out_dir.join(format!("{}.{}", stem, fmt));
    if !result.exists() {
        let log = String::from_utf8_lossy(&out.stderr);
        let brief: Vec<&str> = log.lines().take(6).collect();
        return Err(format!("转换失败：{}", brief.join(" | ")));
    }
    Ok(result.to_string_lossy().to_string())
}
