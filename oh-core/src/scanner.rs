//! 破坏性操作识别：扫描用户指令与 shell 命令，判断是否属于需要审批的操作。

/// 破坏性命令前缀（PowerShell / CMD / 通用）。
const DESTRUCTIVE_PREFIXES: &[(&str, &str)] = &[
    ("rm ", "删除文件/目录 (rm)"),
    ("rm -", "删除文件/目录 (rm)"),
    ("rmdir ", "删除目录 (rmdir)"),
    ("del ", "删除文件 (del)"),
    ("erase ", "删除文件 (erase)"),
    ("Remove-Item", "删除项目 (Remove-Item)"),
    ("rd ", "删除目录 (rd)"),
    ("format ", "格式化磁盘 (format)"),
    ("pip install", "安装 Python 包 (pip install)"),
    ("pip uninstall", "卸载 Python 包 (pip uninstall)"),
    ("pip3 install", "安装 Python 包 (pip3 install)"),
    ("pip3 uninstall", "卸载 Python 包 (pip3 uninstall)"),
    ("python -m pip install", "安装 Python 包 (pip install)"),
    ("move ", "移动/重命名文件 (move)"),
    ("ren ", "重命名文件 (ren)"),
    ("rename ", "重命名文件 (rename)"),
    ("mv ", "移动/重命名文件 (mv)"),
    ("shutil.rmtree", "递归删除目录 (shutil.rmtree)"),
    ("os.remove", "删除文件 (os.remove)"),
    ("os.unlink", "删除文件 (os.unlink)"),
    ("path.unlink", "删除文件 (Path.unlink)"),
    ("shutil.move", "移动/重命名 (shutil.move)"),
    ("os.rename", "重命名 (os.rename)"),
    ("git reset --hard", "硬重置 (git reset --hard)"),
    ("git clean", "清理未跟踪文件 (git clean)"),
];

/// 中文关键词（用于自然语言指令识别）。
const DESTRUCTIVE_KEYWORDS: &[(&str, &str)] = &[
    ("删除", "删除操作"),
    ("删掉", "删除操作"),
    ("删了", "删除操作"),
    ("移除", "删除/移除操作"),
    ("清除", "清除操作"),
    ("格式化", "格式化操作"),
    ("覆盖", "覆盖写操作"),
    ("重命名", "重命名操作"),
    ("改名", "重命名操作"),
    ("移动", "移动文件操作"),
    ("卸载", "卸载操作"),
    ("安装", "安装操作"),
    ("清空", "清空操作"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub pattern: String,
    pub label: String,
}

/// 扫描 shell 命令，返回命中的破坏性模式。
pub fn classify_command(cmd: &str) -> Vec<Match> {
    let lower = cmd.to_lowercase();
    let mut hits = Vec::new();
    for (pat, label) in DESTRUCTIVE_PREFIXES {
        if lower.contains(pat) {
            hits.push(Match { pattern: pat.to_string(), label: label.to_string() });
        }
    }
    hits
}

/// 扫描自然语言指令，返回命中的破坏性关键词。
pub fn classify_instruction(text: &str) -> Vec<Match> {
    let mut hits = Vec::new();
    for (kw, label) in DESTRUCTIVE_KEYWORDS {
        if text.contains(kw) {
            hits.push(Match { pattern: kw.to_string(), label: label.to_string() });
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_destructive_command() {
        let hits = classify_command("rm -rf C:\\work\\x");
        assert!(hits.iter().any(|m| m.label.contains("rm")));
        let hits = classify_command("pip install openpyxl");
        assert!(hits.iter().any(|m| m.label.contains("pip")));
        let hits = classify_command("ls -la");
        assert!(hits.is_empty());
    }

    #[test]
    fn detects_destructive_instruction() {
        let hits = classify_instruction("帮我把这个文件夹删掉");
        assert!(hits.iter().any(|m| m.label.contains("删除")));
        let hits = classify_instruction("帮我把这些 Excel 合并了");
        assert!(hits.is_empty());
    }
}
