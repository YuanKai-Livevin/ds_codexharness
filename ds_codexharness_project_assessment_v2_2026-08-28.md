# ds_codexharness 项目细致评估与后续开发建议（更新复审版）

> 评估对象：`YuanKai-Livevin/ds_codexharness`  
> 当前评估基线：`main` 分支，HEAD `44df52da137ee710137ce982ae95a50888edeaea`  
> 上一版评估基线：`83d2d1a665d04bd4a4ab6ddbb05425dd2ad2697c`  
> 增量范围：上一基线之后 17 个提交，包含 R1–R10、T0 修复、发布包与 README 更新  
> 评估日期：2026-08-28  
> 评估方式：源码静态审查、提交差异审查、模块依赖与数据流分析、测试代码审查  
> 限制：本次没有在目标 Windows 内网环境中运行发行包，没有反编译预编译二进制，也没有用真实模型完成端到端 Golden Tasks。因此，涉及运行时兼容性、Windows 沙箱真实边界、上游模型行为和发行包一致性的结论，仍需通过实机验证确认。

---

## 1. 执行摘要

`ds_codexharness` 的技术路线仍然成立，而且本轮更新不是简单叠加界面功能，而是完成了多项有实质意义的架构改进：

- 将原本集中的 Rust 主应用拆成 `app_state`、`commands/*`、`services/*`；
- 将 Codex app-server 客户端拆成 client、events、approvals、provider 等模块；
- 引入统一引擎状态机；
- 将模型 Gateway 和记忆服务拆为两个独立 Python sidecar；
- sidecar 改用随机端口、运行时令牌和健康握手；
- 修复 Bridge 对官方 Codex 扁平 function 工具结构的识别；
- 修复记忆压缩错误处理全部记忆块的问题；
- 将记忆主存储迁移到 SQLite；
- 新增结构化审计、任务卡、产出物卡和 Diff；
- 新增 SKILLS 的启用、禁用、备份、回滚和测试入口；
- 新增一套确定性 Office 工具，开始从“每次临时写脚本”转向“选工具、填参数、执行和校验”。

这意味着项目已经从：

> **功能覆盖较广但核心链路脆弱的 v0.3 原型**

推进到：

> **具备明确产品架构、可进入受控内部验证的 Beta 候选。**

但当前仍不适合直接大范围部署，也不应立即处理不可恢复、高价值或强合规的企业核心文件。原因不是总体结构错误，而是几条关键链路尚未真正闭环：

1. Bridge 的流式 Responses 事件仍可能生成 Codex 无法解析的 `usage: null` 结构，并且 reasoning、并行工具、非 function 工具仍不完整；
2. 外部导入的 SKILL 测试脚本会被内置 Python 直接执行，尚未进入受限沙箱，这是新增的高风险代码执行入口；
3. Windows junction/symlink 防逃逸只覆盖了部分已存在路径，Office Python 工具又有另一套较弱的 `abspath/commonpath` 边界；
4. 多数 Office 写入工具会直接覆盖同名目标，没有默认冲突拒绝、原子替换和统一审批；
5. sidecar 启动失败、Codex 初始化失败、启动中取消等路径仍可能留下进程和状态竞态；
6. `main` 分支没有保护和 required checks，后端正式测试仍很少，当前功能扩张速度明显快于质量门禁建设速度。

因此，本项目下一阶段不应继续以“增加更多办公工具、更多按钮和更多 Agent 能力”为主，而应先完成：

```text
Bridge 协议一致性
→ SKILL 执行隔离
→ 统一 WorkspaceGuard
→ 安全写入事务
→ Process Supervisor
→ CI / 分支保护 / Golden Tasks
```

本评估的总体建议是：

> **不推倒重来，不 fork Codex Harness。继续把官方 Codex app-server 作为 Agent 执行核心，在外围建设一个更强的产品控制层：统一策略引擎、确定性工具运行时、任务与产出物存储、长期记忆、审计、管理员策略和可靠交付。**

---

## 2. 最终目标与产品北极星

项目最终目标是：

> 使用一个 Harness 工具辅助人员提效。

这个表述方向正确，但如果没有进一步约束，很容易演变为“给聊天框不断加功能”。建议把产品北极星明确为：

> **让普通人员能够用自然语言发起真实工作任务，由 Harness 在明确范围内规划、执行、验证并交付可审查的产出物；用户保留目标、审批和最终接受权。**

### 2.1 用户真正需要的不是“会聊天”，而是“可靠完成工作”

用户关心的是：

- 我交代的任务是否被正确理解；
- Harness 准备读取哪些文件、修改哪些文件；
- 是否会覆盖原文件；
- 是否需要我确认；
- 任务执行到哪一步；
- 结果文件在哪里；
- 结果是否经过验证；
- 失败后能否重试、恢复或回滚；
- 我下次是否可以复用同一流程；
- 使用它是否真的比人工更快、更稳。

因此，聊天应当是入口，但不能成为产品的唯一中心。最终产品应由以下对象共同组成：

```text
对话
任务
执行计划
工具调用
审批
产出物
Diff
验证结果
记忆
审计
```

### 2.2 推荐的产品原则

1. **模型负责理解和规划，确定性工具负责高频执行。**
2. **所有高风险动作由硬策略控制，不依赖提示词自觉。**
3. **输出物优先于聊天文本。**
4. **默认不覆盖，默认可审查，默认可恢复。**
5. **模型可替换，Agent 执行核心可升级，产品控制层保持稳定。**
6. **安全边界只有一套实现，不能让 Rust、Python、Prompt 各自定义。**
7. **技能是可执行插件，不是普通说明文件。**
8. **记忆必须有来源、作用域、可信度、过期和注入预算。**
9. **提效必须被度量，而不是凭主观感觉。**

### 2.3 产品成功指标

建议在审计体系基础上建立以下指标：

- 任务一次完成率；
- 用户接受率；
- 用户不认可原因分布；
- 人工返工次数；
- 平均审批次数；
- 工具首次参数正确率；
- 工具命中率与临时脚本退化率；
- 任务失败后的恢复率；
- 产出物覆盖/误删事件数，目标必须为 0；
- 工作区越界事件数，目标必须为 0；
- 单个成功任务的 Token、耗时和成本；
- 相对人工基线节省的时间；
- SKILL 复用率；
- 阶段交接后任务连续性；
- 用户对结果的最终接受/不接受比例。

---

## 3. 本轮更新概览

从上一评估基线到当前 HEAD，仓库新增或重构了约 70 个文件，形成了 R1–R10 的连续演进。

### 3.1 主要更新对应关系

| 更新主题 | 当前实现 | 评价 |
|---|---|---|
| R1 引擎状态机 | `Stopped/Starting/Ready/Busy/Stopping/Failed` | 方向正确，仍需统一取消与事务清理 |
| R2 双 sidecar | Gateway 与 Memory 分进程、随机端口、令牌、health | 基本修复旧架构问题，但生命周期和 Token 暴露仍需优化 |
| R3 模块化 | Rust 与前端拆分 | 显著改善多人协作和代码可读性 |
| R4 模型能力探测 | `/models`、`/responses`、`/chat/completions` 三探针 | 比旧版更实用，但仍只是浅层可达性探测 |
| R5 SQLite 记忆 | blocks/phases/meta/snapshots | 正确升级，但事务和回滚范围仍不完整 |
| R6 审计 | SQLite `audit.db`，任务/工具/审批/错误 | 价值较高，需完善隐私、保留和领域模型 |
| R7 会话恢复 | 文本、命令输出、文件变更、状态 | 明显增强，但回合边界和 synthetic 消息仍不准确 |
| R8 SKILLS 治理 | 元数据、启停、备份、回滚、测试 | 功能增强，同时引入未隔离脚本执行风险 |
| R9 任务与产出物 | 任务卡、产出物、Diff、接受结果 | 产品方向正确，但依赖审计重建和 live Codex thread |
| R10 Office 工具层 | Excel/Word/PDF/图片/文件工具 | 是正确路线的起点，尚未达到稳定生产工具标准 |

### 3.2 已经实质修复的问题

下列问题可以认为已完成核心修复：

- Bridge 能识别官方 Codex 扁平 function 工具；
- 记忆压缩不再把全部块作为候选；
- 自定义 provider 不再直接使用 URL 作为 TOML table key；
- Python runtime 已补充 FastAPI、Uvicorn、tiktoken 等服务依赖；
- Gateway 与 Memory 不再共享固定 8765 单进程；
- 工作区已拒绝系统目录、驱动器根和 `C:\HARNESS`；
- Rust 和前端大文件完成模块化拆分；
- 记忆主存储迁移到 SQLite；
- 引擎意外停止会进入 Failed，而不是简单显示运行中。

### 3.3 只完成了部分修复的问题

- Bridge 协议：function 工具修了，但完整 Responses 语义没有完成；
- 状态机：状态枚举有了，但启动事务、取消和 sidecar 清理还不统一；
- junction 防护：已存在路径有改善，不存在的输出目标和 Python 工具仍可绕过；
- SKILLS 治理：生命周期增强，但缺少隔离、信任、签名与安全执行；
- 审计：记录能力增强，但隐私、保留、字段级脱敏和任务领域表还不完善；
- Office 工具：具备 schema/dry-run/校验概念，但具体语义和写入安全仍不足；
- 任务与产出物：已有 UI，但底层还依赖审计事件和 Codex thread 临时重建。

### 3.4 本轮新增的主要风险

功能增加后出现了几项上一版不存在或不突出的风险：

1. 导入 SKILL 的 `selftest.py` 可由用户点击直接执行；
2. SKILL frontmatter 的 `name` 被用于路径拼接但未规范化；
3. Office 工具层默认覆盖同名目标；
4. Task/Artifact 依赖 Codex thread，历史删除后关系可能丢失；
5. 审计数据库可能长期保存完整任务目标和路径；
6. sidecar Token 放在 URL query 中；
7. Tauri CSP 仍为 `null`；
8. 内部功能扩张远快于正式测试和 CI 建设。

---

## 4. 当前项目定位

当前项目可定义为：

> **一个基于官方 Codex app-server 的 Windows 本地办公 Agent 产品层，支持内网模型、工作区控制、审批、记忆、任务产出物、审计、SKILLS 和确定性 Office 工具。**

它不是：

- 自己重新实现的 Codex Harness；
- 本地模型推理服务器；
- 已完成的企业知识库；
- 完整复刻官方 Codex App；
- 已经成熟的企业办公平台；
- 可以无审查执行任意第三方技能的安全插件平台。

这个定位是合理的。项目的长期差异化价值不在于重写 Agent Loop，而在于：

- 更适合普通人员的任务界面；
- 企业内网模型适配；
- 工作区和策略控制；
- 高确定性的 Office 工具；
- 产出物、Diff 和接受反馈；
- 长任务的阶段管理；
- 可审计和可治理的技能体系。

---

## 5. 当前架构与数据流

## 5.1 逻辑架构

```text
┌────────────────────────────────────────────────────────────┐
│                    Tauri Desktop UI                        │
│ 对话 / 会话 / 工作区 / 记忆 / SKILLS / 审计 / 任务产出物    │
└────────────────────────────┬───────────────────────────────┘
                             │ Tauri IPC
┌────────────────────────────▼───────────────────────────────┐
│                  Application Control Layer                 │
│ app_state / commands / services                            │
│ 设置、状态机、进程、审批、审计、Sidecar、任务上下文          │
└───────────────┬────────────────────────┬───────────────────┘
                │                        │
                │ JSON-RPC stdio         │ HTTP localhost
                ▼                        ▼
┌──────────────────────────┐   ┌─────────────────────────────┐
│ Official Codex app-server│   │ Memory Sidecar              │
│ Agent Loop / Shell / File│   │ SQLite memory / phase / UI  │
│ Thread / Turn / Sandbox  │   └─────────────────────────────┘
└───────────────┬──────────┘
                │ Responses API
                ▼
      ┌─────────────────────────────┐
      │ Direct Provider or Gateway  │
      │                             │
      │ Direct: upstream /responses │
      │ Bridge: local /responses    │
      │       → /chat/completions   │
      └──────────────┬──────────────┘
                     ▼
             DeepSeek / vLLM / 内网模型

旁路能力：
- Office Tools：Python CLI + SKILL.md
- Audit Store：SQLite audit.db
- Skills Repository：C:\HARNESS\skills
- Workspace Memory：{workspace}\.harness-memory
```

## 5.2 直接模型路径

```text
Codex app-server
→ Provider config.toml
→ 内网/外网 Responses API
```

适用于上游原生支持 `/responses` 的模型服务。

## 5.3 Bridge 模型路径

```text
Codex app-server
→ 本地随机端口 Gateway /responses
→ Responses 请求转换
→ 上游 /chat/completions
→ Chat 流转换回 Responses SSE
→ Codex 继续 Agent Loop
```

Gateway 使用本地运行时令牌鉴权；真实上游 Key 只提供给 Gateway 进程。这是比上一版明显更清晰的信任边界。

## 5.4 记忆数据流

```text
Codex turn/completed usage
→ Rust 写 conversation.json
→ Memory Sidecar 读取水位

用户手工记忆 / 阶段总结
→ FastAPI
→ SQLite memory.db
→ 记忆面板展示

阶段确认
→ 归档旧记忆
→ 可新建 Codex thread
→ 把阶段总结作为新的起始上下文发送
```

当前还没有：

```text
Turn 内容自动抽取
→ 相关性检索
→ 动态注入 Codex Turn
```

因此它仍是阶段笔记和 handoff 系统，而不是完整长期记忆。

## 5.5 Office 工具数据流

```text
模型匹配 office-tools SKILL
→ 生成 otools.py 命令和 JSON 参数
→ Codex Shell 执行 Python CLI
→ 工具做路径校验 / dry-run / 输出校验
→ JSON 结果返回模型
→ 审计记录命令与任务结果
```

这比每次临时生成 Python 脚本更可控，但工具仍然作为 Shell 命令执行，并未成为 Harness 的一等工具。

---

## 6. 设计评价

## 6.1 设计上的优点

### 6.1.1 保留官方 Codex app-server 是正确决策

项目没有重复实现 Agent Loop，而是复用：

- Thread/Turn；
- 工具调用；
- Shell；
- 文件修改；
- 上下文管理；
- Windows sandbox；
- Approval；
- Skills 注册。

这让项目可以把主要精力投入产品化和企业内网能力。当前没有充分理由 fork Codex 内核。

### 6.1.2 从“聊天”开始转向“任务与产出物”

新增任务卡、产出物卡、Diff、接受/不认可，是非常关键的产品方向变化。它开始把系统价值从“模型说了什么”转移到“任务是否完成、文件发生了什么、用户是否接受”。

### 6.1.3 确定性工具路线优于无限制脚本生成

对于 Excel 合并、去重、筛选、PDF 合并、Word 模板等高频任务，固定工具比每次临时写脚本更容易：

- 测试；
- 审批；
- 复用；
- 审计；
- 校验；
- 修复；
- 统计成功率。

这个方向应成为产品的核心，而不是附加技能。

### 6.1.4 双 sidecar 比单体 FastAPI 更合理

模型 Gateway 是关键链路，记忆面板是可选辅助能力。两者拆进程后：

- Gateway 失败可以阻止 Bridge 模式启动；
- 记忆服务失败可以不阻塞 Codex；
- 生命周期和权限可以独立治理；
- 真实上游 Key 不再暴露给记忆 API。

### 6.1.5 SQLite 与结构化审计为后续产品化打下基础

SQLite 让：

- 任务统计；
- 记忆事务；
- 产出物索引；
- 接受率；
- 诊断；
- 数据迁移；
- 本地查询；

具备了可扩展基础。

## 6.2 设计上的不足

### 6.2.1 仍缺少统一 Policy Engine

当前策略分散在：

- Codex approval policy；
- Rust workspace 检查；
- Python Office `resolve()`；
- Prompt 中的工作区铁律；
- SKILL.md 权限文字；
- 前端确认框；
- 工具内部的覆盖逻辑。

结果是同一个概念有多套实现：

```text
“是否允许写某文件”
Rust 判断一次
Python 判断一次
模型判断一次
审批规则再判断一次
```

这些规则不完全一致，容易出现“Rust 认为安全，Python 实际逃逸”或“工具覆盖文件但审批没有触发”。

### 6.2.2 Chat、Task、Audit、Artifact 还没有统一领域模型

当前任务是从审计事件临时拼装，产出物从 Codex thread 重新解析，会话又是 Codex 自己的数据。这些对象之间没有稳定主键和正式关系。

成熟结构应明确：

```text
Workspace
→ Session
→ Task
→ Turn/Run
→ ToolCall
→ Approval
→ Artifact
→ ArtifactVersion
→ Acceptance
```

### 6.2.3 SKILL 被同时当作 Prompt、插件、脚本包和 UI 条目

不同角色的安全要求不同：

- 纯 Prompt 技能可以低风险导入；
- 带脚本技能必须视作代码；
- 带依赖技能需要供应链治理；
- 带网络权限技能需要管理员策略；
- 内置技能和第三方技能需要不同信任级别。

当前治理只完成了元数据和目录管理，没有建立这些层级。

### 6.2.4 “记忆”仍偏名称先行

SQLite 提升了存储可靠性，但没有改变其本质：记忆主要由用户手工维护，阶段总结通过新用户消息送给 Codex。项目不应因为用了 SQLite 就把它宣传为完整长期记忆。

### 6.2.5 产品默认仍向高级调试台倾斜

普通人员不应需要理解：

- Responses API；
- Chat Completions；
- Bridge；
- context token；
- Windows sandbox 实现；
- SKILL 文件目录。

设置页可以保留高级选项，但默认流程应是：

```text
选择模型配置
→ 自动能力探测
→ 自动选择直连/Bridge
→ 选择工作区
→ 开始任务
```

---

## 7. 架构评价

## 7.1 合理之处

### 7.1.1 Rust、Python、前端的职责基本合理

- Rust：桌面壳、进程、DPAPI、Windows API、Codex RPC、策略；
- Python：协议适配、文档工具、记忆服务；
- 前端：任务、会话、审批、文件和审计展示。

### 7.1.2 模块化重构是有效的

`app/src/lib.rs` 已成为装配入口，核心逻辑被拆到 `commands` 和 `services`。Codex client 也拆分审批和事件处理。这大幅降低了多人开发时的冲突。

### 7.1.3 Gateway 使用运行时令牌访问本地端点

Bridge 模式下，Codex 不再直接持有上游真实 Key，而是使用本地令牌连接 Gateway；Gateway 再访问上游。这是正确的秘密边界。

## 7.2 架构问题

### 7.2.1 Process Supervisor 仍不完整

现在有状态机，但缺少统一启动事务、取消、临时子进程所有权和失败回滚。

### 7.2.2 Python Gateway 使用同步 requests 阻塞 async 路由

这会影响并发、关闭、取消和 health 响应。

### 7.2.3 WorkspaceGuard 没有成为跨语言公共能力

Rust 和 Python Office 工具各自实现路径边界。SKILL 导入又有第三套路径拼接。长期必须统一。

### 7.2.4 Task/Artifact 仍依赖 Codex thread

Codex thread 是执行历史，不应成为产品任务数据库。执行引擎升级、thread 删除、格式变化都可能影响产品功能。

### 7.2.5 数据根固定在 `C:\HARNESS`

这会产生：

- 多用户共享目录；
- ACL 和 DPAPI 账户不一致；
- 企业策略冲突；
- 便携与安装模式混淆；
- 卸载、备份和迁移不规范。

---

## 8. 功能评价

## 8.1 对话与会话

### 已完成

- 新建、切换、删除会话；
- 历史文本恢复；
- 命令与输出恢复；
- 文件变更恢复；
- 轮次状态展示；
- 中断 Turn；
- Approval UI。

### 仍需改进

- 连续相同状态的多个回合会被前端合并；
- HistoryMessage 缺少正式 turn_id、item_id、sequence；
- 阶段交接在当前 UI 中显示为 assistant 卡，但历史中仍会还原为 user message；
- 并发工具调用和并发命令输出没有独立缓冲；
- app-server 不运行时，很多历史/产出物功能不可用。

## 8.2 任务与产出物

### 设计价值

这是当前最值得继续投入的产品模块。任务卡已经能显示：

- 目标；
- 状态；
- Token；
- 耗时；
- 成本；
- 工具次数；
- 审批次数；
- 文件变更；
- 用户接受状态。

### 当前局限

- 任务从 audit rows 临时重建；
- 任务读取只取最近有限条审计；
- 产出物依赖 thread/read；
- 没有独立 artifact hash/version；
- 没有产出物验证状态；
- 没有正式回滚；
- 大型文本 Diff 在前端用 O(n×m) LCS，仍可能卡顿；
- Office 二进制文件无法做有意义的文本 Diff。

## 8.3 确定性 Office 工具

### 方向评价

正确，而且应从 SKILL 逐步升级为一等 Tool Runtime。

### 当前能力

- Excel：合并、去重、筛选、简单透视、公式错误值扫描；
- Word：模板填充、文本提取；
- PDF：合并、拆分、文本提取；
- 图片：缩放、转换；
- 文件：清单、重命名；
- 统一 JSON 输出；
- dry-run；
- 错误码；
- 输出可打开校验；
- selftest。

### 当前成熟度

适合内部演示和受控测试，不适合直接标为“稳定工具”。具体语义问题见 T1-08。

## 8.4 SKILLS

### 已完成

- frontmatter 解析；
- version、author、permissions；
- 启用/禁用；
- 导入替换前备份；
- 回滚；
- 测试入口；
- 校验和显示。

### 当前本质问题

当前 SKILL 仍缺少：

- 信任级别；
- 全目录哈希；
- 签名；
- quarantine；
- 权限强制；
- 依赖锁；
- 网络策略；
- 受限执行；
- 来源记录；
- 审核状态。

## 8.5 记忆

### 已完成

- SQLite blocks/phases/meta/snapshots；
- JSON 迁移；
- 手工 CRUD；
- 排序、置顶、暂停；
- 水位；
- 压缩；
- 阶段总结；
- handoff；
- 快照回滚。

### 未完成

- 自动候选提取；
- 来源追踪；
- 相关性检索；
- 自动注入；
- 作用域；
- 过期；
- 冲突；
- 敏感信息分类；
- 使用反馈。

## 8.6 审计

### 已完成

- SQLite 追加记录；
- 任务、工具、文件、审批、错误、引擎事件；
- Token、耗时、成本；
- 接受/不接受；
- 诊断包导出；
- 简单脱敏。

### 未完成

- 保留周期；
- 最大大小；
- 清理与归档；
- 字段级脱敏；
- 敏感任务模式；
- 用户隐私说明；
- ACL；
- 管理员策略；
- 独立 task/artifact 表。

## 8.7 模型能力探测

当前探测能够判断：

- `/models` 是否可达；
- `/responses` 是否返回 200；
- `/chat/completions` 是否返回 200；
- 是否建议启用 Bridge。

但还不能判断：

- 真实 context window；
- function tools 是否稳定；
- parallel tools；
- stream usage；
- reasoning 事件；
- structured output；
- 图片；
- prompt cache；
- Codex 兼容性。

## 8.8 LibreOffice

代码中仍保留打开和转换命令，但构建脚本没有把 LibreOffice 复制到发布目录。当前应视为未闭环功能。

---

## 9. 安全与隐私评价

## 9.1 已有安全能力

- Windows DPAPI 加密 Key；
- 工作区文本扫描；
- 拒绝系统目录和应用数据目录作为工作区；
- Windows final path 尝试；
- Codex Windows sandbox；
- Approval；
- Gateway/Memory Bearer Token；
- 随机端口；
- fail-closed health；
- 审计；
- 诊断包主动导出；
- Office 工具 `root` 边界；
- `file_rename` 默认不覆盖。

## 9.2 主要安全缺口

- SKILL 自测脚本直接执行；
- SKILL name 可参与路径拼接；
- Python Office 路径检查不解析 junction；
- 不存在输出文件的 junction 祖先未解析；
- 大多数 Office 输出会覆盖已有文件；
- Token 进入 URL query；
- Tauri CSP 为 null；
- 原始 harness.log 可能保存敏感 stderr；
- 全权限模式仍对普通用户可见；
- 第三方技能没有来源和签名；
- 依赖没有锁定和哈希；
- 发布 EXE 未签名；
- 审计长期保存任务目标和路径。

## 9.3 安全模型应重新表述

目前 README 中“严格限制”“junction/符号链接防逃逸”等表述强于实际覆盖。更准确的说明应是：

> 应用已经具备多层工作区防护和 Windows sandbox，但对 reparse point、不存在输出路径、第三方技能脚本、Office Python 运行时的边界仍在完善。全权限模式会显著降低保护强度。

---

## 10. 工程质量与交付评价

## 10.1 已改善

- Rust 和前端模块化；
- Cargo 单元测试有所增加；
- Python 有关键回归测试；
- Office 有 selftest；
- Python 服务依赖进入 setup 脚本；
- build 脚本解决 PS5.1 编码和前端嵌入问题；
- Office Tools 进入版本库，不再仅在 vendor。

## 10.2 仍不足

- `main` 无分支保护；
- required checks 为空；
- 没有正式 CI workflow；
- Python backend 正式测试只有少量关键用例；
- 没有 Bridge E2E；
- 没有 Windows junction 集成测试；
- 没有恶意 SKILL 测试；
- 没有进程泄漏测试；
- Python 依赖未固定版本；
- Codex 二进制无哈希验证；
- build 隐藏 Cargo stderr；
- 发布分卷直接进入 Git；
- 没有 LICENSE、THIRD_PARTY_NOTICES、SBOM；
- EXE 无代码签名；
- Release 产物未由 CI 唯一生成。

---

## 11. 更好的总体路线

## 11.1 可选路线

### 路线 A：继续在当前结构上快速堆功能

优点：

- 短期交付快；
- 改动范围小；
- 可以继续演示。

缺点：

- 策略继续分散；
- 测试债务扩大；
- Office 工具越多，覆盖和事务风险越大；
- SKILL 风险会扩散；
- Task/Audit/Artifact 数据继续互相拼装。

不推荐作为长期路线。

### 路线 B：保留 Codex app-server，强化外围产品控制层

主要工作：

- Process Supervisor；
- Policy Engine；
- WorkspaceGuard；
- First-class Tool Runtime；
- Task/Artifact Store；
- Memory Service；
- Audit/Privacy；
- Provider Capability Registry；
- CI/Release Governance。

这是推荐路线。

### 路线 C：fork Codex Harness

只有在以下情况同时成立时才考虑：

- app-server 无法提供必需接口；
- 本地 MCP/client tool 无法满足一等工具；
- 需要改变 Agent Loop、compaction 或 sandbox 内核；
- 团队有能力长期同步上游。

当前不满足这些条件，不推荐。

## 11.2 推荐目标架构

```text
┌───────────────────────────────────────────────────────┐
│                     Product UI                        │
│ Chat / Tasks / Artifacts / Diff / Memory / Audit     │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│                    Orchestrator                       │
│ Session / Task / Run / State / Recovery              │
└─────────────────────────┬─────────────────────────────┘
                          │
┌─────────────────────────▼─────────────────────────────┐
│                    Policy Engine                      │
│ Workspace / Read / Write / Overwrite / Network       │
│ Skill Trust / Approval / Admin Policy                 │
└───────────────┬───────────────────────┬───────────────┘
                │                       │
        ┌───────▼────────┐      ┌───────▼────────────┐
        │ Codex Adapter  │      │ Tool Supervisor   │
        │ app-server RPC │      │ Office/MCP/Worker │
        └───────┬────────┘      └───────┬────────────┘
                │                       │
        ┌───────▼────────┐      ┌───────▼────────────┐
        │ Codex Harness  │      │ Deterministic     │
        │ Agent Loop     │      │ Office Tools      │
        └───────┬────────┘      └────────────────────┘
                │
        ┌───────▼────────┐
        │ Model Gateway  │
        │ Direct/Bridge  │
        └───────┬────────┘
                ▼
          Local / Cloud Model

┌────────────────┬────────────────┬─────────────────────┐
│ Task DB        │ Memory DB      │ Audit DB            │
│ Artifact DB    │ Knowledge      │ Diagnostics         │
└────────────────┴────────────────┴─────────────────────┘
```

## 11.3 核心设计原则

1. Codex 是执行内核，不是产品数据库；
2. 工具必须经过 Policy Engine；
3. 文件写入必须原子化；
4. 第三方 SKILL 默认不可信；
5. Task/Artifact 必须独立存储；
6. Gateway 必须协议测试驱动；
7. Memory 必须检索式注入；
8. 所有发布必须经过自动门禁；
9. 普通用户默认看不到高风险全权限模式；
10. README 的安全描述必须与实际测试覆盖一致。

---

## 12. 成熟度评分

评分范围：1 = 原型，3 = 可受控内部使用，5 = 成熟企业级。

| 维度 | 上一版 | 当前版 | 评价 |
|---|---:|---:|---|
| 产品方向 | 3.0 | 3.8 | 已从聊天向任务/产出物演进 |
| Agent 核心复用 | 4.0 | 4.0 | 继续复用官方 Codex，合理 |
| 模块化 | 1.8 | 3.6 | 本轮改善显著 |
| 模型兼容 | 2.0 | 2.8 | Bridge 有进步，协议仍不完整 |
| 安全边界 | 2.0 | 2.7 | sidecar/路径有进步，SKILL/覆盖新增风险 |
| Office 工具 | 1.5 | 2.8 | 有确定性工具雏形，语义仍需修正 |
| 任务与产出物 | 1.2 | 2.8 | 已有 UI，领域存储未完成 |
| 记忆 | 1.8 | 2.5 | SQLite 提升存储，不是真正长期记忆 |
| 审计与诊断 | 1.0 | 3.0 | 功能已成形，隐私治理不足 |
| 测试与 CI | 1.2 | 1.8 | 有少量测试，主分支无门禁 |
| 构建与供应链 | 1.5 | 2.0 | 依赖补齐，但仍不可复现 |
| 企业部署 | 1.0 | 1.7 | 尚缺策略、签名、更新、ACL |

综合判断：

> **内部 Beta 候选，尚未达到可放心规模部署的 3.0–3.5 区间。**

---

# 13. T0：内部扩大测试前必须完成

## T0-01 Bridge 协议一致性与完成事件

### 现象

Bridge 在上游流式响应没有 usage 时，生成：

```json
"usage": {
  "input_tokens": null,
  "output_tokens": null,
  "total_tokens": null
}
```

而当前 Codex 版本的 `response.completed` usage 结构要求整数。该事件可能被整体判为解析失败，而不是简单显示“水位未知”。

同时还存在：

- 未请求 `stream_options.include_usage`；
- reasoning delta 缺 `summary_index`；
- 并行工具 added/done 事件不完整；
- function name 分片可能丢失；
- custom/freeform/namespace/tool_search 被忽略；
- 图片输入降级为 `[图片]`；
- 上游中断、取消和错误语义不完整。

### 影响

- Turn 无法正常完成；
- Codex 认为 stream prematurely closed；
- 工具调用后无法继续；
- 水位、审计和任务状态不可信；
- 某些模型看似能聊天但不能完成 Agent Loop。

### 修复建议

1. 上游无 usage 时省略 `usage`，不要输出内部 null；
2. 对支持的网关请求 `stream_options.include_usage`；
3. 按 v0.149 Codex parser 建协议 fixture；
4. 补齐 reasoning index 和 tool event 顺序；
5. 对不支持工具直接返回明确能力错误，不要只写日志后继续；
6. Gateway 改为 async HTTP；
7. 建立完整 E2E mock gateway。

### 验收标准

- 无 usage 的流式响应能完成 Turn；
- 单工具和并行工具均能完成两轮 Agent Loop；
- reasoning 可显示或明确禁用；
- 上游 401/429/500/断流能转成正确 Codex 错误；
- 自动测试进入 required checks。

---

## T0-02 第三方 SKILL 执行隔离

### 现象

`test_skill()` 会直接执行 SKILL 中的 `selftest.py` 或 `otools.py selftest`，没有：

- Windows sandbox；
- 网络限制；
- 目录限制；
- 超时；
- Job Object；
- 进程树终止；
- 执行前代码预览。

frontmatter `name` 还被直接用于 backup/enable/disable/test 路径，未做安全 slug 和最终路径校验。

当前“校验和”只计算 `SKILL.md`，使用 FNV-1a 64，不覆盖脚本和资源。

### 影响

- 外来 SKILL 可以在当前用户权限下执行任意代码；
- 可访问网络和工作区外文件；
- 可利用 name 路径逃逸或造成管理错位；
- UI 显示的校验和不能证明技能包完整性。

### 修复建议

1. 导入后先进入 quarantine；
2. 对目录名和 frontmatter name 使用安全 ID；
3. 生成全目录 SHA-256 manifest；
4. 展示来源、文件列表、权限和脚本；
5. 第三方自测默认禁用；
6. 使用受限测试工作区、超时和 kill tree；
7. 禁止网络；
8. 内置签名技能与外部技能分级；
9. 记录信任状态和审核人。

### 验收标准

- 恶意 `selftest.py` 无法访问用户目录、网络和工作区外；
- `..\`、绝对路径和特殊 name 被拒绝；
- Skill manifest 覆盖全部文件；
- 超时后进程树被清理；
- 用户能在执行前查看测试脚本与权限。

---

## T0-03 统一 WorkspaceGuard 与 junction 防逃逸

### 现象

Rust 的 `final_path()` 只在完整路径存在时生效。对于：

```text
workspace\link → junction 到外部
workspace\link\new.xlsx
```

`new.xlsx` 不存在时 final path 失败，代码回退字面比较，可能判为工作区内。

文件递归遍历未拒绝 reparse point。Python Office 工具又只用 `abspath/commonpath`，也无法识别 junction/symlink。

### 影响

- 可读取外部目录；
- 可在外部创建新文件；
- `@` 文件枚举可能遍历外部；
- Office 工具可写出工作区；
- README 中的“严格限制”与真实边界不一致。

### 修复建议

建立唯一 `WorkspaceGuard` 规范：

1. 找到目标最近的已存在祖先；
2. 解析祖先最终路径；
3. 重新附加不存在的后缀；
4. 检查最终祖先仍在工作区；
5. 递归遍历默认不 follow reparse point；
6. Python 工具通过 Rust supervisor 或共享测试规范；
7. SKILL 导入也使用同一 guard。

### 验收标准

真实 Windows junction 测试覆盖：

- 读取；
- 创建；
- 覆盖；
- 重命名；
- 递归枚举；
- Office 输出；
- SKILL 导入；
- `@` 引用。

全部无法越界。

---

## T0-04 Office 安全写入与事务

### 现象

多数工具会直接覆盖同名目标：

- Excel `wb.save()`；
- Word `doc.save()`；
- PDF `open(..., "wb")`；
- 图片 `img.save()`；
- 文本和 CSV 直接写入。

审批规则只看到 `python otools.py ...`，不一定知道工具内部会覆盖。批量重命名逐项执行，中途失败会留下半完成状态。

### 影响

- 静默覆盖用户文件；
- 输出验证失败时原文件可能已损坏；
- 审批无法精确展示将覆盖哪些文件；
- 批处理不可原子回滚。

### 修复建议

- 默认 `overwrite=false`；
- 目标存在返回 `CONFLICT`；
- 显式 overwrite 必须触发 Harness 审批；
- 先写临时文件；
- 完整校验；
- 备份或记录旧 hash；
- 原子 replace；
- 批量操作预计算计划和回滚表；
- 失败时反向恢复。

### 验收标准

- 所有工具默认不覆盖；
- 用户批准后才允许 overwrite；
- 输出校验失败不影响旧文件；
- 批量重命名中途失败可恢复；
- 审计能记录 inputs、outputs、old hash、new hash。

---

## T0-05 Process Supervisor 与启动事务

### 现象

sidecar PID 只有在 health 成功后才写入 state。health 失败时调用 stop，但 state 中还没有 PID，可能留下 Uvicorn 进程。

另外还存在：

- 自动启动和手动启动竞态；
- Starting 中无法统一取消；
- Gateway 启动后 Codex initialize 失败时清理路径不统一；
- Codex 崩溃后 Gateway 不一定立即停止；
- `engine_state` 和 `engine_running` 仍为两份状态；
- pending RPC 在进程退出后可能等待到超时。

### 修复建议

建立启动事务和 supervisor：

```text
Starting
→ spawn gateway（登记临时 PID）
→ health
→ prepare config
→ spawn Codex
→ initialize
→ register skills
→ start thread
→ commit Ready
```

任一步失败：

```text
kill Codex tree
kill Gateway tree
清空 PID/port/token
fail pending RPC
set Failed
```

### 验收标准

- 每一个启动步骤注入失败都无残留进程；
- Starting 可取消；
- 并发 start 只允许一个；
- Codex 崩溃后 Gateway 自动清理；
- pending RPC 立即失败；
- 状态唯一来源，不再依赖镜像布尔值。

---

## T0-06 自动质量门禁

### 现象

`main` 无分支保护，无 required checks。测试数量与功能规模不匹配。

### 修复建议

建立 CI：

```text
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
pytest backend/tests
python tools/office-tools/otools.py selftest
Bridge protocol E2E
Windows junction integration
Malicious skill integration
Process leak integration
Frontend smoke
Release assembly verification
```

并启用：

- PR review；
- branch protection；
- required checks；
- 禁止直接 push main；
- Release 仅由 CI 构建；
- 产物哈希和 SBOM。

### 验收标准

所有 T0 测试作为合并门禁；任何失败不可进入 main。

---

# 14. T1：扩大内部试点前应完成

## T1-01 修复 no-auth 连接测试

当前没有 Key 时直接失败，即使 `no_auth=true`。应允许空 Authorization 正常执行探针。

## T1-02 设置更新改为 Patch API

前端提交不完整 AppSettings，可能清空 `recent_workspaces`。应拆分设置命令，并增加 `clear_api_key`。

## T1-03 使用 TOML 序列化库

当前转义只处理反斜杠和引号，没有处理换行、回车、Tab 和控制字符。Bridge 端口缺失时也不应回退固定端口。

## T1-04 Gateway 改为异步并支持取消

使用 `httpx.AsyncClient`，实现连接/读取/写入超时、客户端断开传播、上游关闭、响应体上限和 backpressure。

## T1-05 前端建立 item 级缓冲

修复 agent delta 未累积、command completed 重复追加、并发命令只维护一个 `currentCmd` 的问题。

## T1-06 SQLite 完整事务与迁移

快照应包含 blocks、phases、meta、conversation；回滚时验证 30 分钟窗口；增加 schema_version、迁移、integrity check 和显式错误。

## T1-07 明确记忆产品定位并建立真实闭环

短期名称可改为“阶段记忆与交接”。长期再实现提取、检索、注入、来源、可信度、冲突、过期和敏感级别。

## T1-08 修复 Office 工具语义

重点问题：

- `.csv/.xls` 被 openpyxl 误处理；
- 输出 `.xls` 实际是 xlsx；
- 多行表头未实现；
- 公式检查不会计算公式；
- PDF 默认拆分并非每页；
- Word 跨 run 占位符失败；
- CSV 未正确转义；
- 多目录同名图片会覆盖；
- 大文件没有内存上限；
- Excel 样式、公式、合并单元格丢失策略未说明。

## T1-09 建立 Task/Artifact 独立数据库

任务完成时立即持久化：

```text
tasks
task_runs
tool_calls
approvals
artifacts
artifact_versions
acceptance
```

不再依赖审计重建和 live thread/read。

## T1-10 正确记录回合边界和 synthetic 消息

HistoryMessage 增加 turn_id、item_id、sequence、synthetic、source。阶段交接应作为结构化 context，不应在历史中重新伪装成普通用户消息。

## T1-11 审计隐私和保留策略

增加：

- 审计开关；
- 保留周期；
- 最大大小；
- 字段级脱敏；
- 敏感任务模式；
- ACL；
- 清理与归档；
- 诊断包预览。

## T1-12 Codex RPC 客户端健壮性

- request timeout 后删除 pending；
- stdout 关闭时 fail all pending；
- 使用 bounded channel；
- 限制命令输出大小；
- 区分 RPC 超时；
- 支持 cancellation；
- 补齐 requestUserInput/MCP/client tool 或明确禁用能力。

## T1-13 Tauri CSP 和本地 Token

开启 CSP；Token 不进入 query；iframe 加 sandbox；限制 connect-src/frame-src；使用 IPC 或 fragment 换取短期 session。

## T1-14 可复现构建和供应链

- 锁定 Python 版本和 wheel hash；
- Codex 二进制 hash；
- Python embeddable hash；
- SBOM；
- THIRD_PARTY_NOTICES；
- LICENSE；
- 不隐藏 Cargo stderr；
- Release 由 CI 生成；
- EXE 签名。

## T1-15 LibreOffice 交付闭环

明确选择系统依赖、便携内置或移除未完成入口。当前代码和构建不一致。

## T1-16 数据根与多用户

将单用户数据迁移至 `%LOCALAPPDATA%`，管理员策略放 `%PROGRAMDATA%`，项目数据留在 workspace。设置 ACL 并提供迁移工具。

## T1-17 Codex 上游版本治理

记录：

- Codex tag/commit；
- app-server schema 版本；
- 安全补丁；
- 兼容矩阵；
- 升级回归；
- rollback 版本。

不能只下载某个 Release 后长期冻结。

---

# 15. T2：平台化能力

## T2-01 一等 Tool Runtime

把 Office 工具从 Shell 命令升级为本地 MCP 或 client tool，由 Rust Tool Supervisor 管理参数、权限、审批、dry-run、输出和回滚。

## T2-02 真正长期记忆

实现候选提取、用户审核、作用域、来源、可信度、相关性检索、预算注入、冲突和过期。

## T2-03 可视化工作流与模板

把高频任务做成：

```text
输入选择
→ 参数表单
→ dry-run
→ 审批
→ 执行
→ 验证
→ 产出物
```

而不必每次都由自然语言完全重新规划。

## T2-04 多模型路由

按任务能力选择模型：

- 文本规划；
- 代码/工具调用；
- OCR/视觉；
- 长上下文；
- 私有模型；
- 低成本模型。

## T2-05 企业管理员策略

管理员可控制：

- 模型地址白名单；
- 网络访问；
- 全权限模式；
- SKILL 导入；
- 工具权限；
- 审计保留；
- 文件类型和大小；
- 数据出网；
- 更新渠道。

## T2-06 提效看板

基于任务和接受数据展示：

- 成功率；
- 接受率；
- 节省时间；
- 工具命中；
- 失败原因；
- 成本；
- 返工；
- 高频任务。

## T2-07 组织知识库与团队共享

在明确权限后，可共享：

- 模板；
- 技能；
- 已验证工作流；
- 术语；
- 常见错误；
- 项目知识。

## T2-08 多模态文档理解

对扫描 PDF、图片表格、流程图、工程图建立专门视觉处理链，不应继续把图片简单替换为 `[图片]`。

## T2-09 任务队列与定时执行

只在安全策略、幂等、失败恢复和管理员治理成熟后再增加后台任务和定时执行。

## T2-10 安装、更新和回滚

建立正式安装器、代码签名、离线更新、数据库迁移、更新前备份、版本回滚和 Release notes。

---

# 16. 合理的后续开发计划

## Milestone A：冻结当前架构基线

### 目标

停止新增大功能，统一问题编号和验收标准。

### 工作项

- 固定当前 commit；
- 建 ADR；
- 建 risk register；
- 建 Golden Tasks；
- 建功能开关；
- 标记实验能力；
- 修正文档中过强的安全表述。

### 出口标准

团队对：

- 产品边界；
- T0/T1/T2；
- 目标架构；
- 测试门禁；

形成书面一致。

## Milestone B：关闭 T0

按依赖顺序实施：

1. Bridge 协议；
2. SKILL 安全；
3. WorkspaceGuard；
4. Office 安全写入；
5. Process Supervisor；
6. CI 与分支保护。

### 出口标准

- Bridge 完成完整工具回路；
- 恶意 SKILL 不可逃逸；
- junction 不可越界；
- 默认不覆盖；
- 启动失败无孤儿进程；
- T0 自动测试全部 required。

## Milestone C：Office 工具可靠化

### 工作项

- 格式能力分开；
- 修复 CSV/XLS/PDF/Word 等语义；
- 原子写入；
- 事务重命名；
- 大文件限制；
- Golden fixtures；
- Office 视觉/格式验证；
- LibreOffice 闭环。

### 出口标准

对每个工具都有：

```text
schema
happy path
边界用例
冲突用例
越界用例
回滚用例
性能上限
```

## Milestone D：Task/Artifact 领域化

### 工作项

- 独立任务数据库；
- 产出物版本；
- hash；
- 验证状态；
- 接受原因；
- 回滚点；
- 不依赖 live thread；
- Office 二进制 Diff 策略。

### 出口标准

关闭 Codex 后仍能查看任务、产出物和审计；删除 thread 不影响产品任务记录。

## Milestone E：审计、隐私与管理员策略

### 工作项

- 数据分类；
- 字段级脱敏；
- 保留周期；
- ACL；
- 管理员策略；
- 诊断包预览；
- 敏感任务模式；
- 全权限模式治理。

### 出口标准

能够回答：

- 记录了什么；
- 为什么记录；
- 保存多久；
- 谁能查看；
- 如何删除；
- 如何导出。

## Milestone F：长期记忆

### 工作项

- 自动候选提取；
- 人工确认；
- 来源和可信度；
- 检索；
- 注入预算；
- 冲突和过期；
- 效果评价。

### 出口标准

记忆注入能提升 Golden Tasks 连续性，并且不会显著增加错误和上下文噪音。

## Milestone G：受控试点

### 工作项

- 选定有限用户；
- 选定有限任务类型；
- 开启回滚与审计；
- 禁用危险全权限和第三方未审核技能；
- 收集任务接受率、节省时间、错误和返工。

### 出口标准

- 无未授权越界；
- 无静默覆盖；
- 失败可恢复；
- 关键任务接受率达到团队设定阈值；
- 用户实际节省时间。

## Milestone H：企业发布

在前述出口全部满足后再进行：

- 正式安装器；
- 代码签名；
- 离线更新；
- 管理员策略；
- 版本回滚；
- SBOM；
- 第三方许可；
- 运营指标。

---

# 17. 推荐 PR 序列

## PR-A1 Bridge Protocol Conformance

- usage；
- reasoning；
- parallel tools；
- async gateway；
- E2E fixture。

## PR-A2 Skill Trust and Sandbox

- safe ID；
- quarantine；
- full manifest；
- restricted selftest；
- timeout/kill tree。

## PR-A3 Unified WorkspaceGuard

- existing ancestor final path；
- no-follow traversal；
- Python/Rust shared cases；
- junction integration。

## PR-A4 Atomic Office Writes

- conflict default；
- overwrite approval；
- temp + validate + replace；
- rollback table。

## PR-A5 Process Supervisor

- start transaction；
- cancellation；
- temporary PID ownership；
- fail pending；
- kill-on-drop。

## PR-A6 CI and Branch Governance

- workflows；
- branch protection；
- required checks；
- release artifact；
- hashes/SBOM。

## PR-B1 Office Semantics

- CSV/XLSX/XLS；
- Word runs；
- PDF split；
- CSV writer；
- image conflicts；
- formula recompute。

## PR-B2 Task and Artifact Store

- tables；
- artifact version；
- acceptance reason；
- offline view。

## PR-B3 Audit Privacy

- field redaction；
- retention；
- ACL；
- sensitive mode；
- export preview。

## PR-C1 Memory Retrieval

在前述可靠性完成后再开始。

---

# 18. Golden Tasks 与故障矩阵

## 18.1 Golden Tasks

1. 合并三个月 Excel，验证行数和表头；
2. 按客户编号去重，验证保留策略；
3. 按条件筛选并生成汇总；
4. Word 模板替换跨 run 占位符；
5. PDF 合并；
6. PDF 每页拆分；
7. 图片多目录同名批量转换；
8. 文件清单包含逗号、中文、换行和引号文件名；
9. 批量重命名发生中途冲突并回滚；
10. 同名输出默认拒绝；
11. 用户批准后覆盖并可恢复；
12. Bridge 单工具完整回路；
13. Bridge 并行工具；
14. 上游不返回 usage；
15. 上游中断；
16. 会话恢复；
17. 任务产出物离线查看；
18. 阶段交接；
19. 记忆压缩不触碰 pinned/paused/new；
20. 用户拒绝审批；
21. Codex 崩溃恢复；
22. Gateway 启动失败；
23. Windows junction 读写逃逸；
24. 恶意 SKILL selftest；
25. 审计敏感信息脱敏。

## 18.2 必测故障

- API Key 错误；
- no-auth；
- 429；
- 500；
- 响应超时；
- SSE 断流；
- usage 缺失；
- 工具 JSON 分片；
- 工具返回超大输出；
- 进程被 taskkill；
- 磁盘满；
- 文件被占用；
- 杀软延迟；
- SQLite 锁；
- 数据库损坏；
- 输出校验失败；
- junction；
- symlink；
- 权限不足；
- SKILL 超时；
- SKILL 启动子进程；
- 应用退出时仍有任务。

## 18.3 评价维度

每个 Golden Task 记录：

- 是否完成；
- 是否一次完成；
- 用户是否接受；
- 输入/输出文件；
- 审批；
- 工具次数；
- Token；
- 耗时；
- 错误；
- 重试；
- 是否回滚；
- 人工基线时间；
- 是否真正提效。

---

# 19. 多人协作与治理建议

## 19.1 模块责任

建议明确代码所有权：

- Codex Adapter；
- Gateway Protocol；
- Workspace/Security；
- Office Tools；
- Skills；
- Memory；
- Task/Artifact；
- Audit/Privacy；
- Frontend；
- Build/Release。

## 19.2 ADR

至少建立：

- 为什么不 fork Codex；
- 为什么使用双 sidecar；
- Policy Engine 边界；
- Task/Artifact 数据模型；
- Skill 信任模型；
- WorkspaceGuard 规范；
- Office 原子写入规范；
- 数据目录；
- Codex 升级策略；
- 审计和隐私策略。

## 19.3 PR 原则

- 一个 PR 只解决一个明确问题；
- 所有 Bug 必须先加回归测试；
- 安全修复必须有攻击测试；
- 不以“README 写了”作为功能完成；
- 不以“selftest 通过”替代 E2E；
- 不在同一 PR 同时做大重构和大功能；
- release commit 必须能追溯到 CI 产物；
- main 禁止直接 push。

---

# 20. Go / No-Go 判断

## 20.1 现在可以做什么

可以：

- 继续共同开发；
- 在隔离测试工作区内做内部验证；
- 验证直连 Responses；
- 验证 Bridge 的基本 function tool；
- 试用 Office 工具但保留原文件；
- 审查任务卡和审计设计；
- 建立 CI、Golden Tasks 和安全测试。

## 20.2 现在不建议做什么

不建议：

- 大规模推广；
- 处理唯一副本和核心业务资料；
- 默认允许覆盖；
- 启用未经审核的第三方 SKILL；
- 普通用户使用全权限模式；
- 把“记忆”宣传为自动长期记忆；
- 把“junction 防逃逸”宣传为完全解决；
- 在没有 CI 的情况下继续快速并入大功能；
- fork Codex Harness。

## 20.3 进入受控试点的最低条件

必须至少完成全部 T0，并证明：

- 工具回路稳定；
- 无静默覆盖；
- 无工作区逃逸；
- 第三方代码不能直接执行；
- 无孤儿进程；
- 任务失败可恢复；
- 发布产物经过自动测试和哈希验证。

---

# 21. 最终结论

本轮更新证明团队能够吸收评估意见并快速调整结构。项目已经出现一个较清晰的长期形态：

```text
官方 Codex Harness
+ 企业内网模型适配
+ 任务与产出物
+ 确定性 Office 工具
+ 工作区和审批
+ 记忆与阶段交接
+ 审计与治理
```

这条路线值得继续开发，也不需要推倒重来。

但需要警惕一个明显趋势：

> **功能增长速度正在超过安全、测试和事务能力增长速度。**

如果继续优先增加更多工具、更多 SKILL、更多 UI，项目会再次积累难以验证的组合风险。下一阶段应以可靠性为中心，优先关闭 Bridge、SKILL、Workspace、写入事务、进程监督和 CI 六条 T0 主线。

推荐的最终定位是：

> **一个以官方 Codex app-server 为执行内核，以统一策略和确定性工具为控制核心，能够帮助人员安全地完成办公任务、交付可验证产出物并持续积累组织能力的 Harness 平台。**

---

# 附录 A：前一版问题修复状态

| 原问题 | 当前状态 |
|---|---|
| Bridge 不识别扁平工具 | 核心已修复，完整协议未完成 |
| 流式 usage/reasoning 不完整 | 部分修复，仍有 null 和 index 问题 |
| 固定 8765、无鉴权 | 基本修复 |
| Provider URL 作为 key | 基本修复 |
| Compactor 候选集错误 | 已修复并有回归测试 |
| 引擎状态不一致 | 部分修复 |
| no-auth 测试 | 未修复 |
| 最近工作区被清空 | 未修复 |
| junction 逃逸 | 部分修复 |
| Python 服务依赖缺失 | 已修复 |
| 大文件模块 | 已显著拆分 |
| 确定性工具缺失 | 已新增，可靠性不足 |
| 任务与产出物缺失 | 已新增，领域存储不足 |
| 审计缺失 | 已新增，隐私治理不足 |
| JSON 主存储 | 已迁移 SQLite |
| SKILL 治理不足 | 部分修复，同时新增执行风险 |
| LibreOffice 未闭环 | 仍未闭环 |
| CI/供应链 | 仍不足 |
| 真正长期记忆 | 未完成 |

---

# 附录 B：重点证据文件索引

本评估重点参考以下当前 HEAD 文件：

```text
app/src/app_state.rs
app/src/lib.rs
app/src/commands/engine.rs
app/src/commands/settings.rs
app/src/commands/workspace.rs
app/src/commands/skills.rs
app/src/commands/memory.rs
app/src/commands/audit.rs
app/src/commands/tasks.rs
app/src/commands/office.rs
app/src/services/memory_sidecar.rs
app/src/services/audit.rs
app/assets/engine.js
app/assets/settings.js
app/assets/memory.js
app/assets/sessions.js
app/assets/tasks.js
app/assets/skills.js
app/assets/boot.js
app/tauri.conf.json
oh-core/src/app_server/client.rs
oh-core/src/app_server/approvals.rs
oh-core/src/provider.rs
oh-core/src/workspace.rs
oh-core/src/prompts.rs
oh-core/src/python.rs
backend/gateway/main.py
backend/api/main.py
backend/api/routes.py
backend/services/auth.py
backend/services/bridge.py
backend/services/compactor.py
backend/services/storage.py
backend/tests/test_regressions.py
tools/office-tools/SKILL.md
tools/office-tools/otools.py
tools/office-tools/otools_lib/common.py
tools/office-tools/otools_lib/excel_tools.py
tools/office-tools/otools_lib/office_tools.py
tools/office-tools/otools_lib/selftest.py
scripts/build.ps1
scripts/setup_python.ps1
README.md
```

---

# 附录 C：建议问题编号

```text
T0-01 Bridge protocol completion
T0-02 Skill trust and sandbox
T0-03 Unified WorkspaceGuard
T0-04 Atomic Office writes
T0-05 Process Supervisor
T0-06 CI and branch protection

T1-01 no-auth probe
T1-02 Settings patch API
T1-03 TOML serializer
T1-04 Async Gateway
T1-05 Frontend item buffers
T1-06 SQLite transaction/migrations
T1-07 Memory product boundary
T1-08 Office semantic correctness
T1-09 Task/Artifact store
T1-10 Turn boundaries/synthetic context
T1-11 Audit privacy/retention
T1-12 RPC robustness
T1-13 CSP/token handling
T1-14 Reproducible build/SBOM
T1-15 LibreOffice closure
T1-16 Per-user data root
T1-17 Codex upstream governance
```
