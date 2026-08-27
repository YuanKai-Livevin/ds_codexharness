//! 工作区沙箱：路径规范化、越界检测、系统目录黑名单。

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
    std::fs::create_dir_all(&canon).map_err(|e| format!("无法创建工作区目录 {}: {}", canon.display(), e))?;
    Ok(canon)
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

/// 判断某个（可能不存在的）路径是否位于工作区内。
pub fn is_within_workspace(workspace: &Path, target: &Path) -> bool {
    let ws = canonicalize_loose(workspace);
    let t = canonicalize_loose(target);
    t.starts_with(&ws)
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
                message: format!("检测到系统级目录「{}」，已拒绝。所有操作仅允许在工作区目录内进行。", d),
            });
        }
    }

    // 2) 目录穿越 ../ 或 ..\
    let mut rest = lower.as_str();
    while let Some(idx) = rest.find("..") {
        // 确认它确实是路径穿越（前后是路径分隔符或边界）
        let before_ok = idx == 0 || matches!(rest.as_bytes()[idx - 1], b' ' | b'/' | b'\\' | b'"' | b'\'' | b'(' | b'\n' | b'\r' | b'\t');
        if before_ok {
            let after = rest[idx + 2..].chars().next();
            if matches!(after, Some('/') | Some('\\')) {
                issues.push(EscapeIssue {
                    snippet: "..".to_string(),
                    message: "检测到目录穿越（../ 或 ..\\），已拒绝。所有操作仅允许在工作区目录内进行。".to_string(),
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
                if ch.is_whitespace() || matches!(ch, '"' | '\'' | '，' | '。' | '；' | '、' | ')' | '】') {
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
    fn escape_scan() {
        let ws = Path::new(r"C:\work\ws");
        let issues = scan_text_for_escapes("把 ../secret.txt 删掉，还有 C:\\Windows\\system32", Some(ws));
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
}
