# JONHON Harness（中航光电 HARNESS）

严谨高效的本地办公自动化桌面工具：基于 OpenAI Codex 引擎 + 内置 Python 3.12，把 AI 助手严格限制在用户指定的「工作区」内执行办公任务（Excel/Word/PPT/PDF/图片批量处理等），并配套权限模式、SKILLS 技能库、记忆面板（上下文水位 / 阶段总结 / 自动压缩交接）等长任务体验增强功能。

- 版本：v0.3.0
- 平台：Windows 10/11（64 位）
- 语言：Rust + TypeScript 无关的纯前端（HTML/CSS/JS）、Python 3.12

---

## 功能特性

| 模块              | 说明                                                         |
| ----------------- | ------------------------------------------------------------ |
| **工作区沙箱**    | 所有文件操作严格限制在工作区内；删除/覆盖/格式转换/安装依赖等危险操作必须经用户确认 |
| **权限模式**      | 只读（靛蓝）/ 读写（青碧绿）/ 全权限（琥珀橘）三档，一键切换并重载引擎 |
| **对话引擎**      | 内置 OpenAI Codex app-server，JSON-RPC 驱动；真实上下文用量实时显示（52K 警戒 / 60K 红线） |
| **SKILLS 技能库** | 固定技能仓库（不可删除），支持自然语言创建/修改/导入技能，助手按匹配度优先调用 |
| **记忆面板**      | 记忆块管理 + 水位监控 + 智能压缩 + **阶段总结**（按阶段工作：确认后旧记忆自动归档、可开启新阶段对话释放上下文）+ 交接文档 |
| **内网免密钥**    | 支持内网部署的 OpenAI 兼容模型，无需 API Key（`requires_openai_auth=false`） |
| **内置翻译层**    | 内网网关仅支持 chat/completions 时，工具本地把 /responses 翻译成 /chat/completions（含流式、推理、工具调用） |
| **品牌定制**      | 应用名 JONHON Harness，顶栏/窗口/任务栏图标均使用 logo       |

---

## 目录结构

```
codexharness/
├── app/                      # Tauri 应用壳（Rust）
│   ├── src/lib.rs            # 全部 Tauri 命令（引擎/沙箱/工作区/会话/记忆/技能）
│   ├── assets/               # 前端资源（index.html / app.js / style.css，构建时嵌入 exe）
│   ├── tauri.conf.json       # Tauri 配置（窗口/图标/ACL）
│   └── capabilities/         # Tauri v2 ACL 权限（core:default + event 监听等）
├── oh-core/                  # 核心逻辑库（Rust）
│   ├── src/codex.rs          # Codex JSON-RPC 驱动（initialize/thread/turn/沙箱/技能）
│   ├── src/config.rs         # 应用设置（settings.json）
│   ├── src/dpapi.rs          # Windows DPAPI 密钥加密（API Key 落盘）
│   ├── src/workspace.rs      # 工作区校验与越界扫描
│   ├── src/scanner.rs        # 破坏性命令分类
│   └── src/winproc.rs        # 进程存活/进程树终止
├── backend/                  # 记忆面板后端（Python FastAPI，交付时随 exe 部署进工作区）
│   ├── api/                  # 路由（记忆 CRUD / 压缩 / 阶段总结 / 交接 / 回滚）
│   ├── models/memory.py      # MemoryBlock 数据模型
│   └── services/             # watermark（52K/60K 水位）/ compactor / handoff / phase / bridge / storage / llm
├── frontend/                 # 记忆面板前端（sidebar.html/css/js，嵌入侧栏 iframe）
├── runtime/python312/        # 内置 Python 3.12（pandas/openpyxl/docx/pptx/pypdf 等办公库）
├── vendor/codex-bin/         # Codex 引擎二进制（app-server / sandbox-setup 等）
├── scripts/                  # 构建与运维脚本
├── dist/OfficeHarness-v0.3/  # 交付包（exe + runtime + codex-bin + memory-block）
└── README.md
```

---

## 构建指南（开发环境）

**前置依赖**

- Rust 工具链（stable，含 cargo）
- PowerShell 5.1+
- 网络可访问 GitHub / DeepSeek（或内网镜像）

**步骤**

```powershell
# 1) 准备内置 Python 运行时（下载 Python 3.12 + 办公库）
powershell -ExecutionPolicy Bypass -File scripts\setup_python.ps1

# 2) 下载 codex 引擎二进制
python scripts\fetch_codex_bins.py

# 3) 一键构建并组装交付包（自动 cargo clean -p office-harness 后 release 构建）
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

构建产物输出到 `dist\OfficeHarness-v0.3\`（含 `OfficeHarness.exe`、`runtime\python312\`、`codex-bin\`、`memory-block\`），并生成同名 zip。

> 注意：Tauri 前端资源在 `generate_context!` 宏展开时嵌入，cargo 指纹不跟踪 assets 变更，因此每次构建必须先 `cargo clean -p office-harness`（build.ps1 已内置）。

**开发运行**

```powershell
$env:OH_DEV_ROOT = (Get-Location).Path
cargo run -p office-harness
```

---

## 运行与使用（最终用户）

1. 解压交付包（保持 exe 与 runtime / codex-bin / memory-block 同目录），双击 `OfficeHarness.exe`；
2. 首次运行：右上角「设置」→ 选择模型供应商、填入 API 基地址与模型名（如内网免密钥勾选「内网部署，无需 API Key」）；
3. 添加工作区：左侧栏「＋ 添加工作区」选择文件夹；
4. 输入办公任务描述即可开始（支持 `@` 引用工作区文件）。

**SmartScreen 提示**：exe 未做代码签名，首次运行若提示"Windows 已保护你的电脑"，点「更多信息 → 仍要运行」。

---

## 内网部署模式

针对内网部署的 OpenAI 兼容模型（如 DeepSeek）：

| 开关                       | 适用场景                                                     |
| -------------------------- | ------------------------------------------------------------ |
| **内网部署，无需 API Key** | 内网网关不校验密钥（`requires_openai_auth=false`）           |
| **使用内置翻译层**         | 内网网关**仅支持 /chat/completions**：工具本地把引擎的 /responses 翻译成 /chat/completions（含流式、推理过程、工具调用），无需外部代理 |

> ⚠️ 引擎（Codex 当前版本）只支持 OpenAI **Responses API**（`/responses`）。若网关原生支持 `/responses`（如 DeepSeek 官方 API、新版 vLLM），无需翻译层；仅支持 `/chat/completions` 时必须开启「使用内置翻译层」。

---

## 数据与配置文件

| 位置                          | 内容                                                         |
| ----------------------------- | ------------------------------------------------------------ |
| `C:\HARNESS\settings.json`    | 应用设置（工作区/模型/密钥密文/沙箱/日志目录）               |
| `C:\HARNESS\codex-home\`      | 引擎配置（config.toml、审批规则 office.policy）、会话历史、内部数据库 |
| `C:\HARNESS\skills\`          | SKILLS 技能库（固定工作区）                                  |
| `C:\HARNESS\logs\harness.log` | 引擎运行日志                                                 |
| `{工作区}\.harness-memory\`   | 记忆面板功能块：data/memory.json（记忆块）、phases.json（阶段记录）、conversation.json（真实水位）、backups/（30 分钟回滚快照） |
| `{工作区}\.oh_tmp\`           | 任务临时文件（可一键清理）                                   |
| `{工作区}\.oh_convert\`       | LibreOffice 转换输出（用到时生成）                           |

旧版本数据位于 `%APPDATA%\OfficeHarness\`，新版本启动时自动迁移到 `C:\HARNESS\`。

---

## 安全模型

- **工作区隔离**：所有文件操作限定在工作区目录内，越界（`../`、系统目录、盘符逃逸）在发送消息时即被拦截；
- **危险操作审批**：`rm/del/format/pip/move` 等命令命中审批规则，必须用户在界面确认后才执行；
- **Windows 沙箱**：默认受限令牌沙箱（unelevated，免 UAC），可选增强沙箱（elevated，需一次 UAC）；
- **密钥加密**：API Key 通过 Windows DPAPI 加密后落盘（仅本机本账户可解密），界面仅脱敏显示（前 6 + 后 4）；
- **网络边界**：仅与配置的模型 API 通信；日志保留在本地。

---

## 常见问题

**Q：引擎无法启动，提示未配置 API Key？**
若为内网免密钥部署，请勾选「内网部署，无需 API Key」；否则在设置中填入有效的 API Key。

**Q：内网网关报 `ResponsesRequest object has no attribute 'chat_template_kwargs'`？**
该报错来自内网 `/responses` 服务端实现不完整（非本工具问题）。确认内网网关仅支持 chat/completions 时，勾选「使用内置翻译层」即可绕开。

**Q：如何清空/重置所有数据？**
关闭应用后删除 `C:\HARNESS\`（全局配置）与各工作区下的 `.harness-memory\`、`.oh_tmp\`；重新启动即回到首次引导。

---

## 交付说明

- 交付物：`dist\OfficeHarness-v0.3.zip`（约 280MB），解压后整体使用；
- 系统要求：Windows 10/11 64 位，Edge WebView2 运行时；
- 建议内网传输，附一页《使用说明》；
- 如需"开箱即用"（内置内网地址/模型/免密钥/翻译层默认值），可在 `oh-core/src/config.rs` 的 `Default` 中预置后重新构建。