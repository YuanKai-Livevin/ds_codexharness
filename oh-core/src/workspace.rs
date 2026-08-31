//! 工作区沙箱：路径规范化、越界检测、系统目录黑名单、最终路径（junction/symlink）防护。

use std::path::{Component, Path, PathBuf};

/// 系统级目录黑名单（无论工作区在哪，这些路径一律禁止触碰）。
const SYSTEM_DIRS: &[&str] = &[
    "C:\\Windows",
    "C:/Windows",
    "C:\\Program Files",
    "C:/Program Files",
    "C:\\Program Files (x86)",
    "C:/Program Files (x86)",
    "C:\\ProgramData",
    "C:/ProgramData",
    "C:\\System Volume Information",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/var",
    "/root",
    "/System",
    "/Library",
    "/Applications",
    "\\\\",
];

/// 禁止选为工作区的目录（含应用自身数据目录，避免把配置/密钥放进工作区）。
const REJECTED_WORKSPACES: &[&str] = &[
    "c:\\windows",
    "c:\\program files",
    "c:\\program files (x86)",
    "c:\\programdata",
    "c:\\harness",
    "c:\\system volume information",
    "c:\\recovery",
    "c:\\$recycle.bin",
    "/",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/var",
    "/root",
    "/system",
    "/library",
    "/applications",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapeIssue {
    /// 命中的原文片段。
    pub snippet: String,
    /// 说明（中文）。
    pub message: String,
}

/// 校验工作区路径本身是否合法，并确保其存在。
pub fn ensure_workspace(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if p.as_os_str().is_empty() {
        return Err("工作区路径不能为空".to_string());
    }
    if !p.is_absolute() {
        return Err("工作区路径必须是绝对路径".to_string());
    }
    if p.is_file() {
        return Err(format!(
            "工作区路径是一个【文件】而不是文件夹：{}\n请重新选择一个文件夹作为工作区。",
            p.display()
        ));
    }
    let canon = canonicalize_loose(&p);
    if is_rejected_workspace(&canon) {
        return Err(format!(
            "该路径是系统目录或应用数据目录，禁止作为工作区：{}\n请选择工作区内的普通文件夹（如 桌面\\xx\\工作文件夹）。",
            canon.display()
        ));
    }
    std::fs::create_dir_all(&canon)
        .map_err(|e| format!("无法创建工作区目录 {}: {}", canon.display(), e))?;
    Ok(canon)
}

/// 判断路径是否属于被拒绝的工作区（盘符根目录 / 系统目录 / 应用数据目录）。
fn is_rejected_workspace(p: &Path) -> bool {
    let canon = canonicalize_loose(p);
    // 盘符根目录（C:\、D:\）或文件系统根（/）
    if canon.file_name().is_none() {
        return true;
    }
    let lower = canon.to_string_lossy().to_lowercase();
    REJECTED_WORKSPACES.iter().any(|r| {
        lower == *r
            || lower.starts_with(&(r.to_string() + "\\"))
            || lower.starts_with(&(r.to_string() + "/"))
    })
}

/// 宽松规范化（不要求路径存在）。
pub fn canonicalize_loose(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Windows：把路径解析为最终路径（解开 junction / symlink / reparse point）。
/// 失败（不存在、权限等）时返回 None，由调用方回退字面比较。
#[cfg(windows)]
pub fn final_path(p: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFinalPathNameByHandleW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        OPEN_EXISTING,
    };

    let wide: Vec<u16> = p
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            0x7, // FILE_SHARE_READ|WRITE|DELETE
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS, // 允许打开目录
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut buf = [0u16; 4096];
        let len = GetFinalPathNameByHandleW(handle, buf.as_mut_ptr(), buf.len() as u32, 0);
        CloseHandle(handle);
        if len == 0 || len as usize >= buf.len() {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..len as usize]);
        // 去掉 \\?\ 前缀（保留大小写规范形式）
        let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
        Some(PathBuf::from(stripped))
    }
}

#[cfg(not(windows))]
pub fn final_path(_p: &Path) -> Option<PathBuf> {
    None
}

/// 供越界判断使用的路径：优先 Windows 最终路径（防 junction/symlink 逃逸），失败回退字面规范化。
fn resolved_for_check(p: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(fp) = final_path(p) {
        return fp;
    }
    canonicalize_loose(p)
}

/// Windows 大小写不敏感前缀判断（带路径边界：前缀后必须是分隔符或结尾）；非 Windows 直接前缀。
pub fn path_within(ws: &Path, t: &Path) -> bool {
    #[cfg(windows)]
    {
        let w = ws.to_string_lossy().to_lowercase();
        let tt = t.to_string_lossy().to_lowercase();
        match tt.strip_prefix(&w) {
            Some(rest) => rest.is_empty() || rest.starts_with('\\') || rest.starts_with('/'),
            None => false,
        }
    }
    #[cfg(not(windows))]
    {
        t.starts_with(ws)
    }
}

/// 判断路径是否为 reparse point（junction / symlink / 挂载点）。
/// 递归遍历时应跳过此类目录，防止跟随链接逃出工作区。
#[cfg(windows)]
pub fn is_reparse_point(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    p.metadata()
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_reparse_point(_p: &Path) -> bool {
    false
}

/// WorkspaceGuard 核心（T0-03）：验证目标位于工作区内，返回可安全使用的最终路径。
///
/// 关键改进：目标路径**不存在**时（如 `workspace\link\new.xlsx`，link 是指向外部的 junction），
/// 逐级向上找最近**已存在**的祖先，解析其最终路径（解开 junction/symlink），
/// 再重新附加不存在的后缀并校验 —— 杜绝「字面比较通过、实际写穿链接」。
pub fn guard_resolve(target: &Path, ws: &Path) -> Result<PathBuf, String> {
    use std::ffi::OsString;
    let mut missing: Vec<OsString> = Vec::new();
    let mut cur: &Path = target;
    loop {
        if cur.exists() {
            let fp = final_path(cur).unwrap_or_else(|| canonicalize_loose(cur));
            let mut out = fp;
            for comp in missing.iter().rev() {
                out.push(comp);
            }
            if !path_within(ws, &out) {
                return Err(format!(
                    "越界：路径解析后位于工作区之外（可能通过 junction/符号链接逃逸）：{}",
                    out.display()
                ));
            }
            return Ok(out);
        }
        match cur.file_name() {
            Some(n) => missing.push(n.to_os_string()),
            None => {
                return Err(format!(
                    "无法解析路径的最近已存在祖先: {}",
                    target.display()
                ));
            }
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => {
                return Err(format!(
                    "无法解析路径的最近已存在祖先: {}",
                    target.display()
                ));
            }
        }
    }
}

/// WorkspaceGuard 对外入口：先规范化工作区自身，再校验目标。
pub fn guard_path(workspace: &Path, target: &Path) -> Result<PathBuf, String> {
    let ws = final_path(workspace).unwrap_or_else(|| canonicalize_loose(workspace));
    guard_resolve(target, &ws)
}

/// 判断某个（可能不存在的）路径是否位于工作区内。
pub fn is_within_workspace(workspace: &Path, target: &Path) -> bool {
    let ws = resolved_for_check(workspace);
    let t = resolved_for_check(target);
    path_within(&ws, &t)
}

/// 扫描一段文本（用户指令 / 命令）中的越界访问企图。
/// `workspace` 为当前工作区绝对路径；盘符路径只有在指向工作区之外时才判定为越界。
pub fn scan_text_for_escapes(text: &str, workspace: Option<&Path>) -> Vec<EscapeIssue> {
    let mut issues = Vec::new();
    let lower = text.to_lowercase();

    // 1) 系统目录黑名单
    for d in SYSTEM_DIRS {
        let dl = d.to_lowercase();
        if lower.contains(&dl) {
            issues.push(EscapeIssue {
                snippet: d.to_string(),
                message: format!(
                    "检测到系统级目录「{}」，已拒绝。所有操作仅允许在工作区目录内进行。",
                    d
                ),
            });
        }
    }

    // 2) 目录穿越 ../ 或 ..\
    let mut rest = lower.as_str();
    while let Some(idx) = rest.find("..") {
        // 确认它确实是路径穿越（前后是路径分隔符或边界）
        let before_ok = idx == 0
            || matches!(
                rest.as_bytes()[idx - 1],
                b' ' | b'/' | b'\\' | b'"' | b'\'' | b'(' | b'\n' | b'\r' | b'\t'
            );
        if before_ok {
            let after = rest[idx + 2..].chars().next();
            if matches!(after, Some('/') | Some('\\')) {
                issues.push(EscapeIssue {
                    snippet: "..".to_string(),
                    message:
                        "检测到目录穿越（../ 或 ..\\），已拒绝。所有操作仅允许在工作区目录内进行。"
                            .to_string(),
                });
                // 跳过这组
                rest = &rest[idx + 2..];
                continue;
            }
        }
        rest = &rest[idx + 2..];
    }

    // 3) 盘符绝对路径：仅在指向工作区之外时判定为越界
    let ws_canon = workspace.map(canonicalize_loose);
    let mut pos = 0;
    let bytes = lower.as_bytes();
    while pos + 2 < bytes.len() {
        let c = bytes[pos] as char;
        if c.is_ascii_alphabetic()
            && bytes[pos + 1] == b':'
            && (bytes[pos + 2] == b'\\' || bytes[pos + 2] == b'/')
        {
            // 提取该路径片段
            let mut end = pos + 2;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if ch.is_whitespace()
                    || matches!(ch, '"' | '\'' | '，' | '。' | '；' | '、' | ')' | '】')
                {
                    break;
                }
                end += 1;
            }
            let seg = &text[pos..end];
            let p = PathBuf::from(seg);
            let outside = match &ws_canon {
                Some(ws) => !is_within_workspace(ws, &p),
                None => true,
            };
            if outside {
                issues.push(EscapeIssue {
                    snippet: seg.to_string(),
                    message: format!("检测到工作区之外的绝对路径「{}」，已拒绝。所有操作仅允许在工作区目录内进行。", seg),
                });
            }
            pos = end;
        } else {
            pos += 1;
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_containment() {
        let ws = Path::new(r"C:\work\ws");
        assert!(is_within_workspace(ws, Path::new(r"C:\work\ws\a\b.xlsx")));
        assert!(!is_within_workspace(ws, Path::new(r"C:\work\ws2\a")));
        assert!(!is_within_workspace(ws, Path::new(r"C:\work\ws\..\other")));
    }

    #[test]
    fn rejected_workspaces() {
        // 盘符根目录 / 系统目录 / 应用数据目录均拒绝
        for bad in [
            r"C:\",
            r"D:\",
            r"C:\Windows",
            r"C:\Windows\System32",
            r"C:\HARNESS",
            r"C:\Program Files",
            "/",
        ] {
            let p = PathBuf::from(bad);
            if p.is_absolute() {
                assert!(is_rejected_workspace(&p), "应拒绝: {}", bad);
            }
        }
        // 普通目录允许
        assert!(!is_rejected_workspace(Path::new(r"C:\work\ws")));
        assert!(!is_rejected_workspace(Path::new(r"F:\桌面文件\项目A")));
    }

    #[test]
    fn ensure_workspace_rejects_system() {
        assert!(ensure_workspace(r"C:\Windows").is_err());
        assert!(ensure_workspace(r"C:\HARNESS").is_err());
        assert!(ensure_workspace(r"C:\").is_err());
    }

    #[test]
    fn escape_scan() {
        let ws = Path::new(r"C:\work\ws");
        let issues = scan_text_for_escapes(
            "把 ../secret.txt 删掉，还有 C:\\Windows\\system32",
            Some(ws),
        );
        assert!(issues.iter().any(|i| i.message.contains("系统级目录")));
        assert!(issues.iter().any(|i| i.message.contains("目录穿越")));
        let issues2 = scan_text_for_escapes("合并工作区里的 a.xlsx 和 b.xlsx", Some(ws));
        assert!(issues2.is_empty());
        // 工作区内的绝对路径不应误报
        let issues3 = scan_text_for_escapes("处理 C:\\work\\ws\\data.xlsx", Some(ws));
        assert!(issues3.is_empty());
        // 工作区外的绝对路径应报
        let issues4 = scan_text_for_escapes("处理 D:\\other\\data.xlsx", Some(ws));
        assert!(issues4.iter().any(|i| i.message.contains("工作区之外")));
    }

    // ---------- T0-03 WorkspaceGuard ----------

    #[test]
    fn guard_path_real_fs() {
        let base = std::env::temp_dir().join(format!("guard-fs-{}", std::process::id()));
        let ws = base.join("ws");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        // 正常：不存在的子路径（祖先存在）应允许
        assert!(guard_path(&ws, &ws.join("sub").join("new.xlsx")).is_ok());
        // 工作区外路径应拒绝
        assert!(guard_path(&ws, &base.join("outside").join("x")).is_err());
        // 目录穿越应拒绝
        assert!(guard_path(&ws, &ws.join("..").join("outside")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 真实 Windows junction 集成测试：`ws\link` junction 指向外部目录，
    /// 已存在文件与不存在的输出路径都必须被拒绝（不许写穿链接）。
    #[cfg(windows)]
    #[test]
    fn guard_rejects_junction_escape() {
        let base = std::env::temp_dir().join(format!("guard-junction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let ws = base.join("ws");
        let outside = base.join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"s").unwrap();
        let link = ws.join("link");
        let out = std::process::Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                link.to_str().unwrap(),
                outside.to_str().unwrap(),
            ])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                // 已存在的路径（经 junction）：必须拒绝
                assert!(
                    guard_path(&ws, &link.join("secret.txt")).is_err(),
                    "已存在文件经 junction 必须被拒绝"
                );
                // 不存在的输出路径（祖先 junction）：必须拒绝
                assert!(
                    guard_path(&ws, &link.join("new.xlsx")).is_err(),
                    "不存在的输出路径经 junction 必须被拒绝"
                );
                // 正常路径不受影响
                assert!(guard_path(&ws, &ws.join("ok.txt")).is_ok());
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
