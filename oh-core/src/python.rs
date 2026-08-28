//! 定位随应用分发的 Python 运行时与辅助文件。

use std::path::{Path, PathBuf};

/// 应用根目录：优先选择包含 runtime/python312 的目录。
/// 依次尝试：exe 所在目录 → OH_DEV_ROOT 环境变量 → 当前工作目录。
pub fn app_root(exe_path: Option<&Path>) -> PathBuf {
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Some(p) = exe_path {
            if let Some(dir) = p.parent() {
                v.push(dir.to_path_buf());
                // exe 可能在 target/debug 下，尝试上溯两级（workspace 根）
                if let Some(ws) = dir.parent().and_then(|d| d.parent()) {
                    v.push(ws.to_path_buf());
                }
            }
        }
        if let Ok(r) = std::env::var("OH_DEV_ROOT") {
            v.push(PathBuf::from(r));
        }
        v.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        v
    };
    for c in &candidates {
        if c.join("runtime").join("python312").join("python.exe").exists() {
            return c.clone();
        }
    }
    candidates
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}

pub struct Bundled {
    pub root: PathBuf,
}

impl Bundled {
    pub fn new(exe_path: Option<&Path>) -> Self {
        Self { root: app_root(exe_path) }
    }

    pub fn python_exe(&self) -> PathBuf {
        self.root.join("runtime").join("python312").join("python.exe")
    }

    pub fn python_dir(&self) -> PathBuf {
        self.root.join("runtime").join("python312")
    }

    pub fn codex_exe(&self) -> PathBuf {
        let dir = self.codex_dir();
        // 优先独立 app-server 二进制，其次主 codex.exe（需加 app-server 子命令）
        let standalone = dir.join("codex-app-server.exe");
        if standalone.exists() {
            standalone
        } else {
            dir.join("codex.exe")
        }
    }

    /// codex 二进制目录（兼容生产布局 codex-bin 与开发布局 vendor/codex-bin）。
    pub fn codex_dir(&self) -> PathBuf {
        let a = self.root.join("codex-bin");
        let b = self.root.join("vendor").join("codex-bin");
        if a.join("codex-app-server.exe").exists() || a.join("codex.exe").exists() {
            a
        } else {
            b
        }
    }

    pub fn sandbox_setup_exe(&self) -> PathBuf {
        self.root.join("codex-bin").join("codex-windows-sandbox-setup.exe")
    }

    /// office-tools 工具包目录（R10；兼容生产布局 office-tools 与开发布局 tools/office-tools）。
    pub fn office_tools_dir(&self) -> PathBuf {
        let a = self.root.join("office-tools");
        if a.join("otools.py").exists() {
            return a;
        }
        let b = self.root.join("tools").join("office-tools");
        if b.join("otools.py").exists() {
            return b;
        }
        let c = self.root.join("vendor").join("office-tools");
        if c.join("otools.py").exists() {
            return c;
        }
        a
    }

    /// LibreOffice 安装目录（兼容生产布局 LibreOffice 与开发布局 vendor/libreoffice/<ver>）。
    pub fn libreoffice_dir(&self) -> PathBuf {
        let a = self.root.join("LibreOffice");
        if a.join("program").join("soffice.exe").exists() {
            return a;
        }
        let b = self.root.join("vendor").join("libreoffice");
        if b.join("program").join("soffice.exe").exists() {
            return b;
        }
        // msiexec /a 管理安装的布局：<root>\LibreOffice\program\soffice.exe 或带版本子目录
        a
    }

    pub fn soffice_exe(&self) -> PathBuf {
        let d = self.libreoffice_dir();
        d.join("program").join("soffice.exe")
    }

    pub fn libreoffice_available(&self) -> bool {
        self.soffice_exe().exists()
    }

    pub fn python_available(&self) -> bool {
        self.python_exe().exists()
    }

    pub fn codex_available(&self) -> bool {
        self.codex_exe().exists()
    }
}
