//! SKILLS 技能治理（R8 元数据/启停/备份/回滚 + T0-02 执行隔离与信任治理）。
//!
//! 安全要点（T0-02）：
//! - 磁盘操作一律使用**目录名**（文件系统安全名），frontmatter name 仅用于展示且必须通过安全校验，
//!   杜绝 `..\`/特殊字符参与路径拼接；
//! - 导入默认进入**隔离区**（disabled-skills/），用户显式「启用」后才进入模型视野；
//! - 全目录 SHA-256 manifest（非仅 SKILL.md），来源/信任/导入时间持久化；
//! - 外部技能测试：先预览脚本 → 确认后在**隔离副本**中运行（无凭据环境、超时、进程树清理）。

use crate::app_state::data_root;
use oh_core::python::Bundled;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillMeta {
    name: String,
    dir_name: String,
    version: String,
    description: String,
    author: String,
    permissions: String,
    /// 全目录 SHA-256（前 16 位展示；完整值在 path 详情）
    checksum: String,
    disabled: bool,
    has_selftest: bool,
    has_backup: bool,
    file_count: usize,
    path: String,
    /// builtin（内置种子技能，信任）| imported（外部导入）
    source: String,
    /// 信任状态：builtin 恒 true；imported 需用户显式启用后为 true
    trusted: bool,
    /// 在隔离区（导入后未启用即处于隔离）
    quarantined: bool,
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
    /// 已进入隔离区，需点「启用」后才被模型使用
    quarantined: bool,
}

#[derive(serde::Serialize, Clone)]
pub(crate) struct SkillTestResult {
    ran: bool,
    exit_code: Option<i32>,
    output_tail: String,
    /// 外部技能首次测试：需要用户确认（附脚本预览）
    need_confirm: bool,
    /// 将要执行的脚本内容预览（确认前展示给用户）
    script_preview: String,
    /// 是否在隔离副本中运行
    sandboxed: bool,
    /// 是否超时被终止（进程树已清理）
    timed_out: bool,
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
    if let Some(after) = trimmed.strip_prefix("---") {
        let end = after.find("---").map(|i| i + 3).unwrap_or(trimmed.len());
        let front = &trimmed[3..end];
        for line in front.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
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

/// 安全目录/ID 名：仅 ASCII 字母数字与 _-.，且不含路径分隔符/遍历符/保留字符。
fn is_safe_dir_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains(['/', '\\'])
        && !s.contains([':', '*', '?', '"', '<', '>', '|'])
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

// ---------- 全目录 SHA-256 manifest ----------

fn hex_of(b: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for x in b {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

mod sha2_like {
    // 轻量 SHA-256（纯 Rust，无外部依赖）——用于技能包完整性校验
    pub struct Sha256 {
        state: [u32; 8],
        buf: [u8; 64],
        buflen: usize,
        len: u64,
    }
    pub trait Digest {
        fn new() -> Self;
        fn update(&mut self, data: &[u8]);
        fn finalize(self) -> [u8; 32];
    }
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    impl Sha256 {
        fn compress(&mut self, block: &[u8]) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    block[i * 4],
                    block[i * 4 + 1],
                    block[i * 4 + 2],
                    block[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ (!e & g);
                let t1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            self.state[0] = self.state[0].wrapping_add(a);
            self.state[1] = self.state[1].wrapping_add(b);
            self.state[2] = self.state[2].wrapping_add(c);
            self.state[3] = self.state[3].wrapping_add(d);
            self.state[4] = self.state[4].wrapping_add(e);
            self.state[5] = self.state[5].wrapping_add(f);
            self.state[6] = self.state[6].wrapping_add(g);
            self.state[7] = self.state[7].wrapping_add(h);
        }
    }
    impl Digest for Sha256 {
        fn new() -> Self {
            Self {
                state: [
                    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                    0x1f83d9ab, 0x5be0cd19,
                ],
                buf: [0; 64],
                buflen: 0,
                len: 0,
            }
        }
        fn update(&mut self, data: &[u8]) {
            self.len += data.len() as u64;
            let mut data = data;
            if self.buflen > 0 {
                let need = 64 - self.buflen;
                let take = need.min(data.len());
                self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
                self.buflen += take;
                data = &data[take..];
                if self.buflen == 64 {
                    let block = self.buf;
                    self.compress(&block);
                    self.buflen = 0;
                }
            }
            while data.len() >= 64 {
                let block: [u8; 64] = data[..64].try_into().unwrap();
                self.compress(&block);
                data = &data[64..];
            }
            if !data.is_empty() {
                self.buf[..data.len()].copy_from_slice(data);
                self.buflen = data.len();
            }
        }
        fn finalize(mut self) -> [u8; 32] {
            let bitlen = self.len.wrapping_mul(8);
            let mut pad = vec![0x80u8];
            let rem = (self.buflen + 1) % 64;
            let zeros = if rem <= 56 { 56 - rem } else { 120 - rem };
            pad.extend(std::iter::repeat_n(0u8, zeros));
            pad.extend_from_slice(&bitlen.to_be_bytes());
            self.update(&pad);
            let mut out = [0u8; 32];
            for (i, v) in self.state.iter().enumerate() {
                out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
            }
            out
        }
    }
}

/// 全目录 SHA-256：按相对路径排序，逐个文件内容混入（上限 3000 文件防异常）。
fn dir_sha256(dir: &Path) -> (String, usize) {
    use sha2_like::Digest as _;
    let mut h = sha2_like::Sha256::new();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(dir, &mut files, 0);
    files.sort();
    let n = files.len();
    for f in files.iter().take(3000) {
        if let Ok(rel) = f.strip_prefix(dir) {
            h.update(rel.to_string_lossy().as_bytes());
            h.update(&[0u8]);
            if let Ok(content) = std::fs::read(f) {
                h.update(&content);
            }
            h.update(&[0u8]);
        }
    }
    (hex_of(&h.finalize()), n)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 16 {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_files(&p, out, depth + 1);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
}

// ---------- manifest 持久化 ----------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SkillManifest {
    dir_name: String,
    source: String,
    trusted: bool,
    imported_at: String,
    dir_hash: String,
}

fn manifest_root() -> PathBuf {
    let d = data_root().join("skill-manifests");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn manifest_path(dir_name: &str) -> PathBuf {
    manifest_root().join(format!("{}.json", dir_name))
}

fn read_manifest(dir_name: &str) -> Option<SkillManifest> {
    std::fs::read_to_string(manifest_path(dir_name))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn write_manifest(m: &SkillManifest) {
    if let Ok(s) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(manifest_path(&m.dir_name), s);
    }
}

fn now_str() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

// ---------- 目录定位（防 frontmatter name 注入） ----------

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

/// 把「展示名」（frontmatter name 或目录名）解析为安全目录名。
/// 遍历启用/禁用两个仓库：匹配目录名或（安全且一致的 frontmatter name）。
fn resolve_dir_name(name: &str) -> Option<String> {
    for repo in [skill_dir(), disabled_dir()] {
        if let Ok(entries) = std::fs::read_dir(&repo) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name == name {
                    return Some(dir_name);
                }
                if let Ok(md) = std::fs::read_to_string(dir.join("SKILL.md")) {
                    let front = parse_frontmatter(&md);
                    if is_safe_dir_name(&front.name) && front.name == name {
                        return Some(dir_name);
                    }
                }
            }
        }
    }
    None
}

fn has_selftest(dir: &Path) -> bool {
    dir.join("selftest.py").exists() || dir.join("otools.py").exists()
}

fn scan_skill(dir: &Path, dir_name: &str, disabled: bool) -> Option<SkillMeta> {
    let md = dir.join("SKILL.md");
    if !md.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&md).ok()?;
    let front = parse_frontmatter(&content);
    // 展示名：frontmatter name 仅当安全时才使用，否则回退目录名
    let name = if is_safe_dir_name(&front.name) {
        front.name.clone()
    } else {
        dir_name.to_string()
    };
    let (dir_hash, fc) = dir_sha256(dir);
    let manifest = read_manifest(dir_name);
    let source = manifest
        .as_ref()
        .map(|m| m.source.clone())
        .unwrap_or_else(|| {
            if dir_name == "office-tools" {
                "builtin".into()
            } else {
                "imported".into()
            }
        });
    let trusted = manifest
        .as_ref()
        .map(|m| m.trusted)
        .unwrap_or(source == "builtin");
    let quarantined = disabled && source == "imported" && !trusted;
    let backups = backup_root().join(dir_name);
    let has_backup = std::fs::read_dir(&backups)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false);
    Some(SkillMeta {
        name,
        dir_name: dir_name.to_string(),
        version: front.version,
        description: front.description,
        author: front.author,
        permissions: front.permissions,
        checksum: dir_hash.chars().take(16).collect(),
        disabled,
        has_selftest: has_selftest(dir),
        has_backup,
        file_count: fc,
        path: dir.to_string_lossy().to_string(),
        source,
        trusted,
        quarantined,
    })
}

/// 读取 SKILLS 仓库：启用技能（skills/）与隔离/禁用技能（disabled-skills/）统一列出。
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

/// 导入技能（T0-02）：默认进入**隔离区**（disabled-skills/），
/// 用户显式「启用」后才被模型使用；写入来源/信任 manifest；同名先备份。
#[tauri::command]
pub(crate) async fn import_skills(dir: String) -> Result<Vec<ImportedSkill>, String> {
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
        let Some(dir_name) = c
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
        else {
            continue;
        };
        // 目录名必须安全（来自文件系统，但二次防御）
        if !is_safe_dir_name(&dir_name) {
            return Err(format!("技能目录名不安全，已拒绝导入: {}", dir_name));
        }
        let content = std::fs::read_to_string(&md).map_err(|e| e.to_string())?;
        let front = parse_frontmatter(&content);
        let name = if is_safe_dir_name(&front.name) {
            front.name.clone()
        } else {
            dir_name.clone()
        };
        // 隔离区目标
        let dst = disabled_dir().join(&dir_name);
        let mut replaced = false;
        if dst.exists() {
            // 备份被替换的旧隔离副本
            let ts = now_str();
            let bk = backup_root().join(&dir_name).join(ts);
            std::fs::create_dir_all(&bk).map_err(|e| e.to_string())?;
            copy_dir_recursive(&dst, &bk).map_err(|e| format!("备份旧版失败: {}", e))?;
            std::fs::remove_dir_all(&dst).map_err(|e| e.to_string())?;
            replaced = true;
        }
        copy_dir_recursive(&c, &dst).map_err(|e| format!("复制 {} 失败: {}", name, e))?;
        // 写入 manifest：外部导入、未信任（隔离中）
        let (dir_hash, _) = dir_sha256(&dst);
        let checksum_short: String = dir_hash.chars().take(16).collect();
        write_manifest(&SkillManifest {
            dir_name: dir_name.clone(),
            source: "imported".into(),
            trusted: false,
            imported_at: now_str(),
            dir_hash,
        });
        imported.push(ImportedSkill {
            name,
            version: front.version,
            description: front.description,
            permissions: front.permissions,
            checksum: checksum_short,
            replaced,
            quarantined: true,
        });
    }
    Ok(imported)
}

/// 禁用技能：移出技能仓库（模型不再看到）。
#[tauri::command]
pub(crate) async fn disable_skill(name: String) -> Result<(), String> {
    let dir_name = resolve_dir_name(&name).ok_or_else(|| format!("技能不存在: {}", name))?;
    let from = skill_dir().join(&dir_name);
    if !from.join("SKILL.md").exists() {
        return Err(format!("技能不存在: {}", name));
    }
    let to = disabled_dir().join(&dir_name);
    if to.exists() {
        std::fs::remove_dir_all(&to).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&from, &to).map_err(|e| format!("禁用失败: {}", e))?;
    Ok(())
}

/// 启用技能：移回技能仓库，并标记为信任（用户显式启用 = 审核通过）。
#[tauri::command]
pub(crate) async fn enable_skill(name: String) -> Result<(), String> {
    let dir_name = resolve_dir_name(&name).ok_or_else(|| format!("技能不存在: {}", name))?;
    let from = disabled_dir().join(&dir_name);
    if !from.join("SKILL.md").exists() {
        return Err(format!("已禁用的技能不存在: {}", name));
    }
    let to = skill_dir().join(&dir_name);
    if to.exists() {
        return Err(format!("技能仓库中已存在同名技能: {}", name));
    }
    std::fs::rename(&from, &to).map_err(|e| format!("启用失败: {}", e))?;
    // 标记信任（用户显式启用）
    if let Some(mut m) = read_manifest(&dir_name) {
        m.trusted = true;
        write_manifest(&m);
    }
    Ok(())
}

/// 测试技能（T0-02）：
/// - 内置技能（trusted）：直接运行，但带超时 + 进程树清理；
/// - 外部技能：首次（confirmed=false）只返回脚本预览要求确认；
///   确认后（confirmed=true）在隔离副本中运行：无凭据环境、30s 超时、进程树清理。
#[tauri::command]
pub(crate) async fn test_skill(
    app: AppHandle,
    name: String,
    confirmed: bool,
) -> Result<SkillTestResult, String> {
    let dir_name = resolve_dir_name(&name).ok_or_else(|| format!("技能不存在: {}", name))?;
    let mut dir = skill_dir().join(&dir_name);
    if !dir.join("SKILL.md").exists() {
        dir = disabled_dir().join(&dir_name);
    }
    if !dir.join("SKILL.md").exists() {
        return Err(format!("技能不存在: {}", name));
    }
    let meta = scan_skill(&dir, &dir_name, dir.starts_with(disabled_dir())).unwrap();
    let bundled = Bundled::new(app.path().resource_dir().ok().as_deref());
    let py = bundled.python_exe();
    let (script_rel, args) = if dir.join("selftest.py").exists() {
        ("selftest.py".to_string(), Vec::<String>::new())
    } else if dir.join("otools.py").exists() {
        (
            "otools.py".to_string(),
            vec![
                "selftest".to_string(),
                "--root".to_string(),
                ".".to_string(),
            ],
        )
    } else {
        return Ok(SkillTestResult {
            ran: false,
            exit_code: None,
            output_tail: "该技能没有内置测试脚本（selftest.py / otools.py）。".into(),
            need_confirm: false,
            script_preview: String::new(),
            sandboxed: false,
            timed_out: false,
        });
    };

    // 外部技能：未确认 → 返回脚本预览要求确认
    if meta.source == "imported" && !confirmed {
        let preview = std::fs::read_to_string(dir.join(&script_rel))
            .unwrap_or_default()
            .chars()
            .take(2000)
            .collect::<String>();
        return Ok(SkillTestResult {
            ran: false,
            exit_code: None,
            output_tail: "外部技能需先确认测试脚本内容，确认后在隔离环境中运行（无凭据、超时 30s、进程树清理）。".into(),
            need_confirm: true,
            script_preview: preview,
            sandboxed: false,
            timed_out: false,
        });
    }

    // 隔离副本：外部技能一律在临时副本运行；内置技能直接在原目录（仍带超时）
    let sandboxed = meta.source == "imported";
    let run_dir: PathBuf = if sandboxed {
        let sandbox = data_root()
            .join(".skill-test")
            .join(format!("{}-{}", dir_name, now_str()));
        let _ = std::fs::create_dir_all(&sandbox);
        copy_dir_recursive(&dir, &sandbox).map_err(|e| format!("准备隔离测试区失败: {}", e))?;
        sandbox
    } else {
        dir.clone()
    };

    let mut cmd = std::process::Command::new(&py);
    cmd.arg(run_dir.join(&script_rel))
        .args(&args)
        .current_dir(&run_dir)
        .env("PYTHONIOENCODING", "utf-8")
        // 隔离环境：不注入任何上游凭据；标记沙箱；清代理
        .env("HARNESS_SANDBOX", "1")
        .env_remove("OH_API_KEY")
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let (exit_code, text, timed_out) = run_timed(cmd, 30);

    // 清理隔离副本（内置技能不删原目录）
    if sandboxed {
        let _ = std::fs::remove_dir_all(&run_dir);
    }

    let lines: Vec<&str> = text.lines().collect();
    let tail: Vec<&str> = lines.iter().rev().take(40).copied().collect();
    let tail = tail.iter().rev().cloned().collect::<Vec<_>>().join("\n");
    Ok(SkillTestResult {
        ran: true,
        exit_code,
        output_tail: tail,
        need_confirm: false,
        script_preview: String::new(),
        sandboxed,
        timed_out,
    })
}

/// 带超时的进程运行：轮询 try_wait，超时后杀进程树（含孙进程），输出合并 stdout+stderr。
fn run_timed(mut cmd: std::process::Command, secs: u64) -> (Option<i32>, String, bool) {
    use std::io::Read;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (None, format!("启动失败: {}", e), false),
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel::<String>();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let tx2 = tx.clone();
    let t1 = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut so) = stdout.take() {
            let _ = so.read_to_end(&mut buf);
        }
        tx.send(String::from_utf8_lossy(&buf).to_string()).ok();
    });
    let t2 = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut se) = stderr.take() {
            let _ = se.read_to_end(&mut buf);
        }
        tx2.send(String::from_utf8_lossy(&buf).to_string()).ok();
    });
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() > deadline {
            timed_out = true;
            oh_core::winproc::kill_tree(pid);
            // 等待进程真正退出
            for _ in 0..20 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut parts = Vec::new();
    while let Ok(s) = rx.recv_timeout(Duration::from_millis(300)) {
        parts.push(s);
    }
    let _ = t1.join();
    let _ = t2.join();
    let _ = status;
    let code = if timed_out {
        None
    } else {
        status.and_then(|st| st.code())
    };
    (code, parts.join("\n"), timed_out)
}

/// 回滚技能：恢复到最近一次导入前的备份版本。
#[tauri::command]
pub(crate) async fn rollback_skill(name: String) -> Result<(), String> {
    let dir_name = resolve_dir_name(&name).ok_or_else(|| format!("技能不存在: {}", name))?;
    let root = backup_root().join(&dir_name);
    let mut backups: Vec<PathBuf> = std::fs::read_dir(&root)
        .map_err(|e| format!("没有该技能的备份: {}", e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    backups.sort();
    let latest = backups
        .pop()
        .ok_or_else(|| "没有可回滚的备份版本。".to_string())?;
    let dst = skill_dir().join(&dir_name);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_dir_name_checks() {
        assert!(is_safe_dir_name("office-tools"));
        assert!(is_safe_dir_name("my_skill.v2"));
        assert!(!is_safe_dir_name(".."));
        assert!(!is_safe_dir_name("a/b"));
        assert!(!is_safe_dir_name("a\\b"));
        assert!(!is_safe_dir_name("a:b"));
        assert!(!is_safe_dir_name(""));
    }

    #[test]
    fn sha256_known_vector() {
        use sha2_like::Digest as _;
        let mut h = sha2_like::Sha256::new();
        h.update(b"abc");
        assert_eq!(
            hex_of(&h.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn dir_hash_stable_across_order() {
        let tmp = std::env::temp_dir().join(format!("skill-hash-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("b.txt"), b"bbb").unwrap();
        std::fs::write(tmp.join("a.txt"), b"aaa").unwrap();
        let (h1, n1) = dir_sha256(&tmp);
        // 再次计算应一致
        let (h2, n2) = dir_sha256(&tmp);
        assert_eq!(h1, h2);
        assert_eq!(n1, 2);
        assert_eq!(n2, 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
