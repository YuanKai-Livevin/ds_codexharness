# JONHON Harness（中航光电 HARNESS）

严谨高效的本地办公自动化桌面工具：基于 OpenAI Codex 引擎 + 内置 Python 3.12，把 AI 助手严格限制在用户指定的「工作区」内执行办公任务（Excel/Word/PPT/PDF/图片批量处理等），配套审批、会话、SKILLS 技能库、记忆面板、结构化审计与确定性办公工具层。

- 版本：v0.3.0
- 平台：Windows 10/11（64 位）
- 语言：Rust（Tauri 2）+ 原生前端（HTML/CSS/JS）、Python 3.12

---

## 功能特性

| 模块 | 说明 |
|---|---|
| **工作区沙箱** | 所有文件操作严格限制在工作区内；`../`、盘符逃逸、系统目录等越界在发送消息时即被拦截；删除/覆盖/格式化/安装依赖等危险操作必须经用户确认 |
| **权限模式** | 只读 / 读写 / 全权限三档，一键切换并重载引擎 |
| **引擎状态机** | 统一状态机（停止/启动中/就绪/忙碌/停止中/失败），引擎崩溃自动检测并提示一键重启 |
| **双 sidecar 架构** | 独立模型网关 + 记忆服务两个 Python 进程，随机端口 + 会话令牌鉴权（fail-closed），互不阻塞 |
| **对话引擎** | 内置 OpenAI Codex app-server，JSON-RPC 驱动；真实上下文用量实时显示（52K 警戒 / 60K 红线） |
| **会话/任务恢复** | 历史会话恢复文本、工具调用（命令+输出，可展开）、文件变更与每回合完成/失败/中断状态 |
| **SKILLS 技能库** | 固定技能仓库；元数据（版本/作者/权限/校验和）、启用/禁用、导入自动备份与回滚、内置测试任务 |
| **记忆面板** | 记忆块管理（SQLite）+ 水位监控 + 智能压缩 + 阶段总结（确认后旧记忆自动归档、可开启新阶段对话释放上下文，交接消息明确标注，不伪装成用户输入） |
| **任务与产出物界面** | 每个任务自动成卡（目标/状态/token/耗时/审批）；产出物卡片支持打开、定位、修改前后 Diff |
| **结构化审计** | SQLite 审计库记录任务目标、模型/网关、工作区、每次审批、工具调用、文件变更、错误、token/耗时/成本与最终是否接受；记录自动脱敏；诊断包由用户主动导出 |
| **确定性办公工具层** | office-tools 技能：13 个稳定工具（Excel 合并/去重/筛选/透视/公式检查、Word 模板填充、PDF 合并/拆分/提取、图片缩放/转换、文件清单/重命名），带参数 schema、权限边界、dry-run 预演、输出校验、错误码与内置测试（20 项） |
| **模型能力自检** | 设置页「测试连接」自动探测 /models、/responses、/chat/completions 三探针，给出翻译层建议 |
| **内网免密钥** | 支持内网部署的 OpenAI 兼容模型，无需 API Key（`requires_openai_auth=false`） |
| **内置翻译层** | 内网网关仅支持 chat/completions 时，本地把 /responses 翻译成 /chat/completions（含流式、推理、工具调用），可开关 |
| **品牌定制** | 应用名 JONHON Harness，顶栏/窗口/任务栏图标使用 logo |

---

## 界面入口

- 左侧栏：任务会话、当前工作区、工作区文件、🧠 记忆面板（内嵌 iframe）、SKILLS 仓库
- 左下角浮动按钮（自上而下）：
  - **📋 任务与产出物**：任务卡（目标/状态/token/审批）+ 产出物（打开/定位/Diff）
  - **🛡 审计日志**：事件流分类筛选 + 一键「导出诊断包」（用户主动，已脱敏）
  - **⚙ 设置**：模型/工作区/权限/内网模式/翻译层/测试连接

---

## 目录结构

```
codexharness/
├── app/                      # Tauri 应用壳（Rust）
│   ├── src/lib.rs            # 装配入口（thin）
│   ├── src/app_state.rs      # 全局状态 + 引擎状态机
│   ├── src/commands/         # 命令模块（settings/workspace/engine/sessions/skills/memory/audit/tasks/office）
│   ├── src/services/         # 记忆 sidecar（随机端口/令牌） + 审计存储（SQLite）
│   └── assets/               # 前端资源（index.html / 模块化 JS / style.css，构建时嵌入 exe）
├── oh-core/                  # 核心逻辑库（Rust）
│   ├── src/app_server/       # Codex JSON-RPC 客户端 / 事件分发 / 审批
│   ├── src/config.rs         # 应用设置（settings.json）
│   ├── src/provider.rs       # CODEX_HOME config.toml 生成（含翻译层 base_url）
│   ├── src/dpapi.rs          # Windows DPAPI 密钥加密
│   ├── src/workspace.rs      # 工作区校验与越界扫描（junction/符号链接防逃逸）
│   ├── src/scanner.rs        # 破坏性命令分类
│   ├── src/prompts.rs        # 开发者指令（工具优先方法论）
│   └── src/python.rs         # 内置运行时定位（python/codex/office-tools）
├── backend/                  # 记忆服务后端（Python FastAPI，随 exe 部署）
│   ├── api/                  # 路由（记忆 CRUD / 压缩 / 阶段总结 / 交接 / 回滚，令牌鉴权）
│   └── services/             # watermark / compactor / handoff / phase / bridge / auth / llm
├── frontend/                 # 记忆面板前端（sidebar.html/css/js，嵌入侧栏 iframe）
├── tools/office-tools/       # 确定性办公工具包（otools.py + otools_lib + SKILL.md，版本库管理）
├── runtime/python312/        # 内置 Python 3.12（openpyxl/docx/pptx/pypdf/PIL 等办公库）
├── vendor/codex-bin/         # Codex 引擎二进制（app-server / sandbox-setup 等）
├── scripts/                  # 构建与运维脚本（build.ps1 / setup_python.ps1）
├── dist/OfficeHarness-v0.3/  # 交付包（exe + runtime + codex-bin + memory-block + office-tools）
└── release/                  # 分卷包（GitHub 100MB 限制拆分，合并.bat 一键还原）
```

---

## 构建指南（开发环境）

**前置依赖**：Rust 工具链（stable）、PowerShell 5.1+、可访问 GitHub / DeepSeek（或内网镜像）。

```powershell
# 1) 准备内置 Python 运行时（下载 Python 3.12 + 办公库）
powershell -ExecutionPolicy Bypass -File scripts\setup_python.ps1

# 2) 下载 codex 引擎二进制
python scripts\fetch_codex_bins.py

# 3) 一键构建并组装交付包（自动 cargo clean -p office-harness 后 release 构建）
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

构建产物输出到 `dist\OfficeHarness-v0.3\`，并生成同名 zip（约 280MB）。

> 注意：Tauri 前端资源在 `generate_context!` 宏展开时嵌入，cargo 指纹不跟踪 assets 变更，因此每次构建必须先 `cargo clean -p office-harness`（build.ps1 已内置）。另外 build.ps1 须以 **UTF-8 + BOM** 保存（Windows PowerShell 5.1 会按 ANSI 误读无 BOM 的中文注释导致解析错乱）。

**开发运行**：

```powershell
$env:OH_DEV_ROOT = (Get-Location).Path
cargo run -p office-harness
```

---

## 运行与使用（最终用户）

1. 下载 `release/` 下 4 个分卷，双击 `合并.bat` 还原 `OfficeHarness-v0.3.zip`（或直接使用 `dist\OfficeHarness-v0.3\`）；
2. 解压后保持 exe 与 runtime / codex-bin / memory-block / office-tools 同目录，双击 `OfficeHarness.exe`；
3. 首次运行：右下角「设置」→ 模型供应商、API 基地址、模型名（内网免密钥勾选「内网部署，无需 API Key」）→ 测试连接；
4. 左侧栏「＋ 添加工作区」选择文件夹；
5. 输入办公任务描述即可开始（支持 `@` 引用工作区文件）。

**SmartScreen 提示**：exe 未做代码签名，首次运行若提示"Windows 已保护你的电脑"，点「更多信息 → 仍要运行」。

---

## 内网部署模式

| 开关 | 适用场景 |
|---|---|
| **内网部署，无需 API Key** | 内网网关不校验密钥（`requires_openai_auth=false`） |
| **使用内置翻译层** | 内网网关**仅支持 /chat/completions**：本地把引擎的 /responses 翻译成 /chat/completions（含流式、推理过程、工具调用），无需外部代理 |

> ⚠️ 引擎（Codex 当前版本）只支持 OpenAI **Responses API**（`/responses`）。若网关原生支持 `/responses`（如 DeepSeek 官方 API、新版 vLLM），无需翻译层；仅支持 `/chat/completions` 时必须开启「使用内置翻译层」。设置页「测试连接」可自动探测网关能力。

---

## 数据与配置文件

| 位置 | 内容 |
|---|---|
| `C:\HARNESS\settings.json` | 应用设置（工作区/模型/密钥密文/沙箱/日志目录/内网开关） |
| `C:\HARNESS\codex-home\` | 引擎配置（config.toml、审批规则 office.policy）、会话历史 |
| `C:\HARNESS\skills\` | SKILLS 技能库（office-tools 自动种子） |
| `C:\HARNESS\disabled-skills\` | 已禁用的技能（随时可恢复） |
| `C:\HARNESS\skill-backups\` | 技能导入前自动备份（可回滚） |
| `C:\HARNESS\audit\audit.db` | 结构化审计库（SQLite，追加式） |
| `C:\HARNESS\logs\harness.log` | 引擎运行日志 |
| `{工作区}\.harness-memory\` | 记忆面板功能块：backend（服务）/ frontend / data（memory.db 记忆块、conversation.json 真实水位、memory.json 导出备份） |
| `{工作区}\.oh_tmp\` | 任务临时文件（可一键清理） |
| `{工作区}\.oh_convert\` | LibreOffice 转换输出（用到时生成） |

旧版本数据位于 `%APPDATA%\OfficeHarness\`，新版本启动时自动迁移到 `C:\HARNESS\`。

---

## 安全模型

- **工作区隔离**：所有文件操作限定在工作区目录内；junction/符号链接防逃逸（`GetFinalPathNameByHandle` 校验）；驱动器根、系统目录等被拒绝作为工作区；
- **危险操作审批**：`rm/del/format/pip/move` 等命令命中审批规则，必须用户在界面确认后才执行；每次审批决策写入审计；
- **Windows 沙箱**：默认受限令牌沙箱（unelevated，免 UAC），可选增强沙箱（elevated，需一次 UAC）；
- **密钥加密**：API Key 通过 Windows DPAPI 加密后落盘（仅本机本账户可解密），界面仅脱敏显示（前 6 + 后 4）；
- **sidecar 鉴权**：模型网关与记忆服务使用每次启动随机生成的会话令牌，健康检查 fail-closed，CORS 仅允许本应用；
- **审计与脱敏**：任务/审批/工具/文件变更/错误全程记录；含密钥关键词的日志与审计内容自动脱敏，诊断包只由用户主动导出；
- **确定性工具**：office-tools 所有路径强制工作区内校验，带 dry-run 与输出校验，工具执行同样纳入审计。

---

## 常见问题

**Q：引擎无法启动，提示未配置 API Key？**
若为内网免密钥部署，请勾选「内网部署，无需 API Key」；否则在设置中填入有效的 API Key。

**Q：内网网关报 `ResponsesRequest object has no attribute 'chat_template_kwargs'`？**
该报错来自内网 `/responses` 服务端实现不完整（非本工具问题）。确认内网网关仅支持 chat/completions 时，勾选「使用内置翻译层」即可绕开。

**Q：发消息一直失败/超时？**
先检查网络与代理：本工具只向设置的模型 API 发起请求。若本机代理未运行（如系统代理指向 127.0.0.1:10809 但代理软件未启动），外部 HTTPS 会全部失败。

**Q：如何清空/重置所有数据？**
关闭应用后删除 `C:\HARNESS\`（全局配置）与各工作区下的 `.harness-memory\`、`.oh_tmp\`；重新启动即回到首次引导。

---

## 交付说明

- 交付物：`dist\OfficeHarness-v0.3.zip`（约 280MB），解压后整体使用；
- GitHub 分卷：`release\OfficeHarness-v0.3.zip.001 ~ 004`（各 90/90/90/10.1MB），下载后双击 `合并.bat` 还原；
- 系统要求：Windows 10/11 64 位，Edge WebView2 运行时；
- 如需"开箱即用"（内置内网地址/模型/免密钥/翻译层默认值），可在 `oh-core/src/config.rs` 的 `Default` 中预置后重新构建。
