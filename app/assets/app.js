/* 办公自动化助手 前端逻辑 */
"use strict";

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
const listen = TAURI.event.listen;

// ---------- 状态 ----------
const state = {
  settings: null,
  apiKey: "",              // API Key 仅保存在内存
  running: false,
  streaming: false,
  currentAssistant: null,   // 正在流式输出的气泡元素
  currentCmd: null,         // 正在输出的命令块
  currentReason: null,      // 思考过程气泡
  approvalQueue: [],        // 待处理的审批请求队列
  pendingApproval: null,    // 当前展示中的审批 { requestId, kind }
  workspace: "",            // 工作区绝对路径
  fs: { current: null },    // 文件浏览器状态
  refs: [],                 // 对话引用的工作区文件（相对路径）
  mentionQuery: "",         // @ 面板过滤词
  mentionFiles: [],         // 工作区全部文件（相对路径）
  sessions: [],             // 当前工作区的会话列表
  currentSessionId: null,   // 当前会话 id
  skillsRepo: "",           // SKILLS 仓库路径
};

// ---------- 工具 ----------
function $(sel) { return document.querySelector(sel); }

// 精致的线条图标（内联 SVG，随文字颜色，hover 变红）
const ICON_DELETE =
  '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M3 6h18"/><path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2"/>' +
  '<path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/></svg>';
const ICON_X =
  '<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>';

function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function toast(msg, type) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "toast" + (type ? " " + type : "");
  clearTimeout(t._timer);
  t._timer = setTimeout(() => t.classList.add("hidden"), 4200);
}

function addMsg(kind, innerHtml, extraClass) {
  const wrap = document.createElement("div");
  wrap.className = "msg " + kind + (extraClass ? " " + extraClass : "");
  const bubble = document.createElement("div");
  bubble.className = "bubble";
  bubble.innerHTML = innerHtml;
  wrap.appendChild(bubble);
  $("#messages").appendChild(wrap);
  scrollBottom();
  updateChatEmptyBg();
  return bubble;
}

// 空态背景：没有任何问答消息时显示 logo 铺满，有消息即隐藏
function updateChatEmptyBg() {
  const bg = $("#chat-empty-bg");
  if (!bg) return;
  const msgs = $("#messages");
  const hasMsg = msgs && msgs.querySelectorAll(":scope > .msg").length > 0;
  bg.classList.toggle("hidden", hasMsg);
}

function scrollBottom() {
  const m = $("#messages");
  m.scrollTop = m.scrollHeight;
}

// 简易 Markdown 渲染（加粗/行内代码/代码块/列表/换行）
function renderMd(text) {
  let html = escapeHtml(text);
  // 代码块
  html = html.replace(/```([\s\S]*?)```/g, (m, code) =>
    '<div class="cmd-block">' + code.trim() + "</div>"
  );
  // 行内代码
  html = html.replace(/`([^`\n]+)`/g, '<code style="background:#eeece6;color:#c2401a;padding:1px 5px;border-radius:4px;font-family:var(--mono);font-size:12px;">$1</code>');
  // 加粗
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  // 列表
  html = html.replace(/^(\s*)[-*] (.+)$/gm, "$1• $2");
  return html;
}

// 将助手文本按标签分段渲染
function renderAssistantText(bubble, text) {
  const tagMap = {
    "执行计划": ["tag-plan", "📋 执行计划"],
    "危险操作警告": ["tag-warn", "⚠ 危险操作警告"],
    "任务完成": ["tag-done", "✅ 任务完成"],
  };
  const parts = text.split(/(【[^】]+】)/g);
  bubble.innerHTML = "";
  let tagStack = [];
  for (const part of parts) {
    const m = part.match(/^【([^】]+)】$/);
    if (m && tagMap[m[1]]) {
      const [cls, title] = tagMap[m[1]];
      const el = document.createElement("div");
      el.className = "tag-block " + cls;
      const t = document.createElement("div");
      t.className = "tag-title";
      t.textContent = title;
      el.appendChild(t);
      bubble.appendChild(el);
      tagStack.push(el);
    } else {
      const el = document.createElement("div");
      el.innerHTML = renderMd(part);
      if (tagStack.length) {
        tagStack[tagStack.length - 1].appendChild(el);
      } else {
        bubble.appendChild(el);
      }
    }
  }
}

// ---------- 事件处理 ----------
async function initEvents() {
  if (!window.__TAURI__ || !window.__TAURI__.event) {
    showFatalError("前端桥接失败：window.__TAURI__ 不可用（withGlobalTauri 未生效）。请重新下载最新版本。");
    return;
  }
  await listen("oh-event", (e) => {
    const ev = e.payload;
    try {
      handleEvent(ev);
    } catch (err) {
      console.error("[oh-event handler error]", err);
      showFatalError("界面事件处理出错：" + (err && err.message ? err.message : err));
    }
  });
}

// 界面级错误横幅（出现任何前端错误时可见，便于排查）
function showFatalError(msg) {
  const bar = document.getElementById("fatal-bar");
  if (!bar) {
    const div = document.createElement("div");
    div.id = "fatal-bar";
    div.style.cssText = "position:fixed;top:0;left:0;right:0;background:#f25a5a;color:#fff;z-index:999;padding:8px 14px;font-size:13px;font-family:Consolas,monospace;white-space:pre-wrap;";
    document.body.prepend(div);
    div.textContent = "⚠ " + msg;
  } else {
    bar.textContent = "⚠ " + msg;
  }
}

function handleEvent(ev) {
  switch (ev.type) {
    case "status":
      setEngineChip(ev.state, ev.detail);
      if (ev.state === "running") {
        state.running = true;
      }
      if (ev.state === "error") {
        state.running = false;
        toast(ev.detail || "引擎启动失败", "err");
        // 引导用户补全配置
        if (ev.detail && ev.detail.includes("API Key")) openSettings();
      }
      break;
    case "log":
      console.log("[engine]", ev.level, ev.msg);
      if (ev.level === "error") {
        if (!handleEngineError(ev.msg)) toast("引擎错误：" + ev.msg, "err");
      }
      break;
    case "threadStarted":
      toast("会话已建立");
      break;
    case "turnStarted":
      state.streaming = true;
      state.currentCmd = null;
      showRunBar("任务运行中…", null);
      break;
    case "agentDelta":
      if (!state.currentAssistant) {
        state.currentAssistant = addMsg("assistant", "");
      }
      renderAssistantText(state.currentAssistant, ev.text);
      scrollBottom();
      break;
    case "agentMessage":
      if (!state.currentAssistant) {
        state.currentAssistant = addMsg("assistant", "");
      }
      renderAssistantText(state.currentAssistant, ev.text);
      scrollBottom();
      break;
    case "reasoningDelta":
      if (!state.currentReason) {
        state.currentReason = addMsg("assistant", "", "reasoning");
        state.currentReason.innerHTML =
          '<div class="r-head">🧠 思考摘要（点击展开）</div><div class="r-body"></div>';
        state.currentReason.querySelector(".r-head").addEventListener("click", () => {
          state.currentReason.classList.toggle("open");
        });
      }
      state.currentReason.querySelector(".r-body").textContent += ev.text;
      break;
    case "commandStarted": {
      // 命令块默认折叠：只显示一行命令，点击展开输出（减少噪音）
      const wrap = document.createElement("div");
      wrap.className = "cmd-wrap";
      if (ev.dangerous && ev.dangerous.length) {
        const warn = document.createElement("div");
        warn.className = "tag-block tag-warn";
        warn.style.margin = "4px 0";
        const t = document.createElement("div");
        t.className = "tag-title";
        t.textContent = "⚠ 破坏性操作：" + ev.dangerous.join("、");
        warn.appendChild(t);
        wrap.appendChild(warn);
      }
      const head = document.createElement("div");
      head.className = "cmd-head";
      head.textContent = "💻 " + (ev.command || "").slice(0, 140) + (ev.command.length > 140 ? "…" : "");
      head.title = ev.command + "\n（点击展开/收起输出）";
      const out = document.createElement("div");
      out.className = "cmd-output";
      out.style.display = "none";
      head.addEventListener("click", () => {
        out.style.display = out.style.display === "none" ? "" : "none";
        if (state.currentCmd && state.currentCmd.itemId === ev.itemId) {
          state.currentCmd._open = out.style.display !== "none";
        }
        scrollBottom();
      });
      wrap.append(head, out);
      $("#messages").appendChild(wrap);
      state.currentCmd = { wrap, out, itemId: ev.itemId, cmd: ev.command };
      scrollBottom();
      break;
    }
    case "commandOutput":
      if (state.currentCmd && state.currentCmd.itemId === ev.itemId) {
        state.currentCmd.out.textContent += ev.output;
        state.currentCmd._hasOut = true;
        if (state.currentCmd._open) {
          scrollBottom();
        }
      }
      break;
    case "commandCompleted":
      if (state.currentCmd && state.currentCmd.itemId === ev.itemId) {
        if (ev.output) {
          state.currentCmd.out.textContent += ev.output;
        }
        const ok = ev.status === "completed";
        const head = state.currentCmd.wrap.querySelector(".cmd-head");
        if (head) {
          head.textContent = (ok ? "✔ " : "✘ ") + (state.currentCmd.cmd || ev.command || "").slice(0, 140);
          head.title = (state.currentCmd.cmd || ev.command || "") + "\n（点击展开/收起输出）";
        }
        state.currentCmd = null;
        scrollBottom();
      }
      break;
    case "fileChangeStarted": {
      const wrap = document.createElement("div");
      wrap.className = "msg assistant";
      const b = document.createElement("div");
      b.className = "bubble";
      b.innerHTML = '<div class="tag-block tag-plan"><div class="tag-title">📝 文件修改</div><div class="cmd-block">' +
        escapeHtml(ev.summary) + "</div></div>";
      wrap.appendChild(b);
      $("#messages").appendChild(wrap);
      scrollBottom();
      break;
    }
    case "fileChangeCompleted":
      break;
    case "approvalRequest":
      state.approvalQueue.push(ev);
      showNextApproval();
      break;
    case "approvalResolved":
      // 服务端已关闭的请求（例如被中断）：从队列中移除
      state.approvalQueue = state.approvalQueue.filter((a) => a.requestId !== ev.requestId);
      if (state.pendingApproval && state.pendingApproval.requestId === ev.requestId) {
        state.pendingApproval = null;
        $("#modal-approval").classList.add("hidden");
        showNextApproval();
      }
      break;
    case "turnCompleted": {
      state.streaming = false;
      if (state.currentReason) {
        state.currentReason.querySelector(".r-head").textContent = "🧠 思考摘要（点击展开）";
        state.currentReason = null;
      }
      state.currentAssistant = null;
      // 任务完成标识：不依赖模型输出，系统自动给出
      const statusLabel = ev.status === "completed" ? "已完成" : (ev.status || "已结束");
      const ok = ev.status === "completed";
      // 上下文长度（输入 tokens ≈ 上下文长度）
      let ctxTokens = null;
      let usageLine = "";
      try {
        const u = typeof ev.usage === "string" ? JSON.parse(ev.usage) : ev.usage;
        if (u) {
          const inp = u.input_tokens ?? u.total_token_usage?.input_tokens;
          const out = u.output_tokens ?? u.total_token_usage?.output_tokens;
          ctxTokens = inp != null ? inp : null;
          usageLine = (inp != null ? " · 输入 " + inp + " token" : "") + (out != null ? " · 输出 " + out + " token" : "");
        }
      } catch (e) { /* ignore */ }
      // 更新上下文显示（加载框与常驻小字）
      updateContext(ctxTokens);
      updateMemBar(ctxTokens);
      showRunBar(ok ? "✔ 任务完成" : "⚠ 任务" + statusLabel, ctxTokens);
      const done = addMsg("assistant", "");
      const tag = document.createElement("div");
      tag.className = "tag-block " + (ok ? "tag-done" : "tag-warn");
      const title = document.createElement("div");
      title.className = "tag-title";
      title.textContent = ok ? "✅ 任务完成" : "⚠ 任务" + statusLabel;
      tag.appendChild(title);
      const line = document.createElement("div");
      line.textContent = "本任务已结束" + usageLine;
      tag.appendChild(line);
      const btnRow = document.createElement("div");
      btnRow.style.marginTop = "6px";
      const btn = document.createElement("button");
      btn.className = "btn btn-small";
      btn.textContent = "打开工作区文件夹";
      btn.addEventListener("click", () => invoke("open_workspace").catch((e) => toast(String(e), "err")));
      btnRow.appendChild(btn);
      tag.appendChild(btnRow);
      done.appendChild(tag);
      scrollBottom();
      // 任务结束后刷新会话列表（更新预览）
      setTimeout(loadSessions, 1500);
      break;
    }
    case "engineStopped":
      state.running = false;
      setEngineChip("off", "");
      hideRunBar();
      break;
    case "sandboxSetupResult": {
      const ok = ev.success;
      const modeLabel = ev.mode === "elevated" ? "增强沙箱" : "受限令牌沙箱";
      toast(ok ? modeLabel + "配置成功" : modeLabel + "配置失败：" + (ev.error || "未知错误"), ok ? "" : "err");
      addMsg("assistant",
        '<div class="tag-block ' + (ok ? "tag-done" : "tag-warn") + '">' +
        '<div class="tag-title">' + (ok ? "✅ " + modeLabel + "已就绪" : "⚠ " + modeLabel + "配置失败") + "</div>" +
        (ev.error ? escapeHtml(ev.error) : "") + "</div>");
      break;
    }
    case "unknown":
      console.log("[engine][unknown]", ev.method);
      break;
  }
}

// ---------- 运行状态条（加载框 + 上下文长度） ----------
function fmtTokens(n) {
  if (n == null) return "--";
  if (n >= 1000) return (n / 1000).toFixed(1) + "k";
  return String(n);
}

function showRunBar(text, ctxTokens) {
  const bar = $("#run-bar");
  if (!bar) return;
  bar.classList.remove("hidden");
  const t = $("#run-text");
  if (t) t.textContent = text || "任务运行中…";
  const c = $("#run-ctx");
  if (c) {
    // 上下文长度仅在取到具体值时显示
    if (ctxTokens != null) {
      c.textContent = "上下文 " + fmtTokens(ctxTokens) + " tokens";
    } else {
      c.textContent = "";
    }
  }
  clearTimeout(bar._timer);
  if (text && text.indexOf("✔") === 0) {
    // 完成后短暂显示再隐藏
    bar._timer = setTimeout(() => bar.classList.add("hidden"), 4000);
  }
}

function hideRunBar() {
  const bar = $("#run-bar");
  if (bar) bar.classList.add("hidden");
}

function updateContext(ctxTokens) {
  // 上下文长度仅在取到值时展示在运行状态条；取不到值则留空（已移除常驻显示）
  const c = $("#run-ctx");
  if (c && ctxTokens != null) {
    c.textContent = "上下文 " + fmtTokens(ctxTokens) + " tokens";
  }
}

// ---------- 审批弹窗 ----------
function showNextApproval() {
  if (state.pendingApproval) return;
  const ev = state.approvalQueue.shift();
  if (!ev) return;
  state.pendingApproval = { requestId: ev.requestId, kind: ev.kind };
  $("#ap-reason").textContent = ev.reason || "引擎请求执行以下操作：";
  $("#ap-kind").textContent = ev.kind === "command" ? "命令执行" : "文件修改";
  $("#ap-kind").className = "chip chip-warn";
  if (ev.kind === "command") {
    $("#ap-command").textContent = ev.command || "(空命令)";
    $("#ap-command").classList.remove("hidden");
    $("#ap-changes").classList.add("hidden");
  } else {
    $("#ap-changes").textContent = ev.changes || "(无变更摘要)";
    $("#ap-changes").classList.remove("hidden");
    $("#ap-command").classList.add("hidden");
  }
  $("#modal-approval").classList.remove("hidden");
}

async function resolveApproval(decision) {
  const p = state.pendingApproval;
  if (!p) return;
  state.pendingApproval = null;
  $("#modal-approval").classList.add("hidden");
  try {
    await invoke("respond_approval", { requestId: p.requestId, decision });
  } catch (e) {
    toast("审批响应失败：" + e, "err");
  }
  showNextApproval();
}

// ---------- 引擎状态 ----------
// 引擎状态（引擎已全自动，仅控制发送按钮可用性）
function setEngineChip(stateStr, detail) {
  const sendBtn = $("#btn-send");
  if (stateStr === "starting") {
    if (sendBtn) sendBtn.disabled = true;
  } else if (stateStr === "error") {
    if (sendBtn) sendBtn.disabled = true;
  } else if (stateStr === "running" || state.running) {
    if (sendBtn) sendBtn.disabled = false;
  } else {
    if (sendBtn) sendBtn.disabled = true;
  }
  void detail;
}

// 权限模式切换（只读 / 读写 / 全权限）
const MODE_MAP = {
  read: { sandbox: "read-only", theme: "read", label: "只读模式" },
  write: { sandbox: "workspace-write", theme: "write", label: "读写模式" },
  danger: { sandbox: "danger-full-access", theme: "danger", label: "全权限模式" },
};

function modeFromSandbox(sandbox) {
  if (sandbox === "read-only") return "read";
  if (sandbox === "danger-full-access") return "danger";
  return "write";
}

function renderMode(mode) {
  const sw = $("#mode-switch");
  if (!sw) return;
  sw.dataset.mode = mode;
  document.body.dataset.theme = MODE_MAP[mode].theme;
  document.querySelectorAll(".mode-option").forEach((o) => {
    o.classList.toggle("active", o.dataset.mode === mode);
  });
}

async function switchMode(mode) {
  if (!MODE_MAP[mode]) return;
  const cur = modeFromSandbox((state.settings && state.settings.sandbox_mode) || "workspace-write");
  if (cur === mode) return;
  const m = MODE_MAP[mode];
  // 油漆铺满动画（1.5s）
  paintOverlay(m.theme);
  renderMode(mode);
  // 保存设置 + 重启引擎
  const s = await invoke("get_settings").catch(() => null);
  if (s) {
    s.sandbox_mode = m.sandbox;
    try {
      await invoke("save_settings", { settings: s });
      state.settings = s;
      toast("已切换至" + m.label);
      const noAuth = !!(s.no_auth);
      if (state.apiKey || (await invoke("has_api_key").catch(() => false)) || noAuth) {
        state.running = false;
        startEngine();
      }
    } catch (e) {
      toast("模式切换失败：" + e, "err");
    }
  }
}

function paintOverlay(theme) {
  // 纯主题主色（不渐变）、低透明度
  const color = {
    read: "#365B94",
    write: "#2A9390",
    danger: "#C45C26",
  }[theme] || "#2A9390";
  const btn = $("#mode-switch");
  const r = btn ? btn.getBoundingClientRect() : { left: innerWidth / 2, top: 40, width: 10, height: 10 };
  const cx = r.left + r.width / 2;
  const cy = r.top + r.height / 2;
  const maxR = Math.hypot(Math.max(cx, innerWidth - cx), Math.max(cy, innerHeight - cy));
  const overlay = document.createElement("div");
  overlay.className = "paint-overlay";
  overlay.style.left = cx + "px";
  overlay.style.top = cy + "px";
  overlay.style.setProperty("--max-r", Math.ceil(maxR * 2) + "px");
  overlay.style.background = color;
  document.body.appendChild(overlay);
  requestAnimationFrame(() => {
    requestAnimationFrame(() => overlay.classList.add("expand"));
  });
  setTimeout(() => {
    document.body.dataset.theme = theme;
    setTimeout(() => {
      overlay.classList.add("fade");
      setTimeout(() => overlay.remove(), 500);
    }, 80);
  }, 800);
}

// ---------- 刷新状态 ----------
async function refreshStatus() {
  try {
    const s = await invoke("get_status");
    state.running = s.running;
    state.workspace = s.workspace || "";
    setEngineChip(s.running ? "running" : "off", "");
    if (!s.python_ok || !s.codex_ok) {
      toast("缺少运行组件：请把本程序与 runtime/、codex-bin/ 放在同一目录", "warn");
    }
  } catch (e) {
    console.error("refreshStatus", e);
  }
}

// ---------- SKILLS 仓库 ----------
async function renderSkills() {
  const list = $("#skills-list");
  if (!list) return;
  try {
    const info = await invoke("get_skills_repo");
    state.skillsRepo = info.path || "";
    list.innerHTML = "";
    if (!info.skills.length) {
      list.innerHTML = '<div class="fs-empty">还没有技能。点「＋」创建或导入。</div>';
      return;
    }
    for (const sk of info.skills) {
      const item = document.createElement("div");
      item.className = "skill-item";
      item.title = sk.path;
      item.innerHTML =
        '<span class="skill-icon">🧩</span>' +
        '<div class="skill-body"><div class="skill-name">' + escapeHtml(sk.name) + "</div>" +
        (sk.description ? '<div class="skill-desc">' + escapeHtml(sk.description) + "</div>" : "") + "</div>";
      list.appendChild(item);
    }
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">技能仓库读取失败：' + escapeHtml(String(e)) + "</div>";
  }
}

// ---------- 认证错误提示（Key 失效等） ----------
function handleEngineError(msg) {
  const lower = String(msg).toLowerCase();
  if (lower.includes("401") || lower.includes("unauthorized") || lower.includes("invalid") || lower.includes("authentication") || lower.includes("api key")) {
    toast("⚠ API Key 无效或已过期，请到「设置」更新", "err");
    addMsg("assistant",
      '<div class="tag-block tag-warn"><div class="tag-title">⚠ API Key 需要更新</div>' +
      "模型服务返回认证失败（401）。请在右上角「设置」中填入最新 API Key 并保存，引擎会自动重启。" +
      "</div>");
    openSettings();
    return true;
  }
  return false;
}

// ---------- 工作区文件浏览器 ----------
function fsFormatSize(n) {
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(1) + " GB";
  if (n >= 1 << 20) return (n / (1 << 20)).toFixed(1) + " MB";
  if (n >= 1 << 10) return (n / (1 << 10)).toFixed(0) + " KB";
  return n + " B";
}

function fsCrumbText(path) {
  const ws = state.workspace.replace(/\\+$/, "");
  if (!path || path === ws) return "📁 " + (ws.split(/[\\/]/).pop() || ws);
  return "📁 " + path.slice(ws.length).replace(/^[\\/]/, "").replace(/[\\/]/g, " / ");
}

// ---------- 工作区切换（dsh 风格：SKILLS 仓库固定 + 我的工作区） ----------
async function renderWorkspacePanel() {
  const s = await invoke("get_settings").catch(() => null);
  if (!s) return;
  state.settings = s;
  // 若工作区路径是文件而非文件夹，给出醒目警告
  const warn = $("#ws-warn");
  if (s.workspace_path) {
    const r = await invoke("list_dir", { path: s.workspace_path }).catch((e) => String(e));
    if (typeof r === "string" && r.includes("不是目录")) {
      warn.textContent = "⚠ 当前工作区路径不是文件夹，请重新添加工作区。";
      warn.classList.remove("hidden");
    } else {
      warn.classList.add("hidden");
    }
  }
  const recentEl = $("#ws-recent");
  recentEl.innerHTML = "";

  // 固定项：SKILLS 仓库（永远存在、不可删除）
  let skillsPath = state.skillsRepo;
  if (!skillsPath) {
    try {
      const info = await invoke("get_skills_repo");
      skillsPath = info.path || "";
    } catch (e) { /* ignore */ }
  }
  if (skillsPath) {
    recentEl.appendChild(wsItem(skillsPath, "🧩", skillsPath === s.workspace_path, true));
  }

  // 固定项：记忆面板（记忆块管理 + 上下文压缩/交接 + 真实水位）
  const memItem = document.createElement("div");
  memItem.className = "ws-item mem-item";
  memItem.innerHTML =
    '<span class="ws-icon">🧠</span>' +
    '<span class="ws-name">记忆面板</span>' +
    '<span class="ws-sub">上下文水位 · 压缩 · 交接</span>';
  memItem.title = "打开记忆面板：记忆块管理、自动压缩与交接文档（当前工作区）";
  memItem.addEventListener("click", () => openMemoryPanel());
  recentEl.appendChild(memItem);

  // 我的工作区列表：当前工作区 + 已添加的其他工作区
  const all = [s.workspace_path].concat((s.recent_workspaces || []).filter((r) => r && r !== s.workspace_path));
  const uniq = [];
  for (const w of all) {
    if (w && w !== skillsPath && !uniq.includes(w)) uniq.push(w);
  }
  if (uniq.length) {
    const label = document.createElement("div");
    label.className = "ws-label";
    label.textContent = "我的工作区";
    recentEl.appendChild(label);
  }
  for (const w of uniq) {
    recentEl.appendChild(wsItem(w, w === s.workspace_path ? "📌" : "📁", w === s.workspace_path, false));
  }
}

function wsItem(path, icon, isCurrent, isFixed) {
  const item = document.createElement("div");
  item.className = "ws-item" + (isCurrent ? " ws-current" : "");
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  item.innerHTML =
    '<span class="ws-icon">' + icon + "</span>" +
    '<span class="ws-name">' + escapeHtml(isCurrent ? name + (isFixed ? "（固定）" : "（当前）") : name) + "</span>" +
    (isFixed ? "" : '<span class="sess-actions"><button class="del-btn ws-del" title="删除此工作区">' + ICON_DELETE + "</button></span>");
  item.title = path;
  if (!isCurrent) {
    item.addEventListener("click", () => switchWorkspace(path));
  }
  const del = item.querySelector(".ws-del");
  if (del) {
    del.addEventListener("click", (ev) => {
      ev.stopPropagation();
      deleteWorkspace(path);
    });
  }
  return item;
}

// 打开记忆面板（记忆块管理 + 上下文压缩/交接 + 真实水位）
async function openMemoryPanel() {
  try {
    const st = await invoke("memory_status").catch(() => null);
    if (st && !st.running) {
      toast("记忆服务未运行，面板将显示演示数据。");
    }
    await invoke("open_memory_panel");
  } catch (e) {
    toast("打开记忆面板失败：" + e, "err");
  }
}

// ---------- 内嵌记忆面板（侧栏 iframe + 真实水位条 + 阶段联动） ----------
const MEMORY_ORIGIN = "http://127.0.0.1:8765";

async function setupMemoryCard() {
  const frame = $("#memory-frame");
  const offline = $("#mem-offline");
  if (!frame) return;
  try {
    const st = await invoke("memory_status").catch(() => null);
    if (st && st.running) {
      if (!frame.src || frame.src.indexOf(MEMORY_ORIGIN) === -1) {
        frame.src = MEMORY_ORIGIN + "/";
      }
      offline.classList.add("hidden");
      refreshMemoryCount();
    } else {
      frame.removeAttribute("src");
      offline.classList.remove("hidden");
    }
  } catch (e) { /* 保持占位 */ }
}

// 记忆面板高度自适应：
//   空 → 与「工作区文件」卡片同高；有块 → 最多显示 3 张卡片，再多内部滚动
function updateMemoryFrameHeight() {
  const wrap = $("#memory-frame-wrap");
  if (!wrap) return;
  const fp = document.querySelector(".file-panel");
  const fileH = fp ? fp.offsetHeight : 160;
  const n = state.memoryCount;
  let h;
  if (n == null || n === 0) {
    h = fileH;                        // 空态：与工作区文件窗口同高
  } else {
    h = Math.min(n, 3) * 82 + 74;     // 3 张卡 + 底部按钮 + 间距
  }
  h = Math.max(140, Math.min(h, 400));
  wrap.style.height = h + "px";
}

async function refreshMemoryCount() {
  try {
    const r = await fetch(MEMORY_ORIGIN + "/api/memory/blocks", { timeout: 3000 }).catch(() => null);
    if (!r) return;
    const d = await r.json().catch(() => null);
    if (d && Array.isArray(d.blocks)) {
      state.memoryCount = d.blocks.length;
      updateMemoryFrameHeight();
    }
  } catch (e) { /* 保持当前高度 */ }
}

// 主窗口水位条（与记忆面板同源数据：codex 每轮 input tokens）
function updateMemBar(ctxTokens) {
  const bar = $("#mem-bar");
  const txt = $("#mem-text");
  const wm = $("#mem-wm");
  if (!bar || ctxTokens == null) return;
  const max = 60000;
  const warn = 52000;
  const pct = Math.min(100, (ctxTokens / max) * 100);
  bar.style.width = pct + "%";
  bar.className = "mem-progress-fill " +
    (ctxTokens >= max ? "critical" : ctxTokens >= warn ? "warn" : "");
  txt.textContent = "上下文 " + fmtTokens(ctxTokens) + " / 60k tokens";
  if (wm) {
    wm.textContent = ctxTokens >= max ? "超红线" : ctxTokens >= warn ? "警戒区" : "正常";
    wm.className = "mem-wm " +
      (ctxTokens >= max ? "wm-critical" : ctxTokens >= warn ? "wm-warn" : "wm-ok");
  }
}

// 阶段确认联动：面板确认「同时开启新阶段对话」后，新建会话并把阶段总结作为首条消息
async function startPhaseThread(firstMessage) {
  const text = (firstMessage || "").trim();
  if (!text) { toast("没有阶段总结内容，已跳过新对话", "warn"); return; }
  try {
    if (!state.running) {
      const started = await startEngine();
      if (!started) return;
    }
    await invoke("new_session");
    state.currentSessionId = null;
    clearMessages();
    updateMemBar(0); // 新会话 = 全新上下文
    addMsg("user", renderMd(text));
    await invoke("send_message", { text });
    toast("新阶段对话已开启（首条消息 = 阶段总结）", "ok");
    await loadSessions();
  } catch (e) {
    toast("开启新阶段对话失败：" + e, "err");
  }
}

// 删除工作区（当前工作区删除后自动切换到下一个）
async function deleteWorkspace(path) {
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  if (!confirm("删除工作区「" + name + "」？（只从列表移除，不会删除文件夹）")) return;
  try {
    const newWs = await invoke("remove_workspace", { path });
    if (newWs) {
      // 删的是当前工作区：刷新状态并重启引擎
      toast("已删除工作区，正在切换到 " + newWs + "…");
      if (state.running) await invoke("stop_engine");
      state.running = false;
      setEngineChip("off", "");
      await refreshStatus();
      renderWorkspacePanel();
      refreshFS();
      const noAuth = !!(state.settings && state.settings.no_auth);
      if (state.apiKey || (await invoke("has_api_key").catch(() => false)) || noAuth) {
        startEngine();
        setTimeout(loadSessions, 8000);
      } else {
        loadSessions();
      }
    } else {
      renderWorkspacePanel();
      toast("已删除工作区");
    }
  } catch (e) {
    toast("删除失败：" + e, "err");
  }
}

// ---------- 会话管理（dsh 式：按工作区划分的任务会话） ----------
function clearMessages() {
  // 保留空态背景元素，只清除问答消息
  const m = $("#messages");
  m.querySelectorAll(":scope > .msg").forEach((el) => el.remove());
  state.currentAssistant = null;
  state.currentCmd = null;
  state.currentReason = null;
  state.approvalQueue = [];
  state.pendingApproval = null;
  updateChatEmptyBg();
}

async function loadSessions() {
  if (!state.running) return;
  try {
    const sessions = await invoke("list_sessions");
    const cur = await invoke("current_session").catch(() => null);
    state.sessions = sessions || [];
    state.currentSessionId = cur || null;
    renderSessions();
  } catch (e) {
    console.error("loadSessions", e);
  }
}

function sessionLabel(s) {
  const p = (s.preview || "").trim();
  if (p) return p.slice(0, 40);
  return "对话 " + (s.id ? s.id.slice(0, 8) : "");
}

function renderSessions() {
  const list = $("#session-list");
  if (!list) return;
  list.innerHTML = "";
  if (!state.sessions.length && !state.currentSessionId) {
    list.innerHTML = '<div class="fs-empty">还没有会话，点「＋ 新对话」开始</div>';
    return;
  }
  const hasCurrent = state.sessions.some((s) => s.id === state.currentSessionId);
  // 当前会话是尚未发消息的新对话：显示占位
  if (state.currentSessionId && !hasCurrent) {
    const item = document.createElement("div");
    item.className = "session-item session-current";
    item.innerHTML = '<span class="sess-icon">●</span><span class="sess-name">新对话（发送消息后保存）</span><span class="sess-actions"></span>';
    list.appendChild(item);
  }
  for (const s of state.sessions) {
    const item = document.createElement("div");
    const isCur = s.id === state.currentSessionId;
    item.className = "session-item" + (isCur ? " session-current" : "");
    item.innerHTML =
      '<span class="sess-icon">' + (isCur ? "●" : "💬") + "</span>" +
      '<span class="sess-name">' + escapeHtml(sessionLabel(s)) + "</span>" +
      '<span class="sess-actions"><button class="del-btn sess-del" title="删除会话">' + ICON_DELETE + "</button></span>";
    item.title = s.id;
    if (!isCur) {
      item.addEventListener("click", () => switchSession(s.id));
    }
    const del = item.querySelector(".sess-del");
    del.addEventListener("click", (ev) => {
      ev.stopPropagation();
      deleteSession(s.id);
    });
    list.appendChild(item);
  }
}

async function newSession() {
  if (!state.running) { toast("引擎未启动，请稍候…", "warn"); return; }
  try {
    await invoke("new_session");
    clearMessages();
    updateMemBar(0); // 新会话 = 全新上下文
    toast("已创建新对话");
    await loadSessions();
  } catch (e) {
    toast("新建会话失败：" + e, "err");
  }
}

async function switchSession(id) {
  if (id === state.currentSessionId) return;
  try {
    await invoke("switch_session", { threadId: id });
    state.currentSessionId = id;
    clearMessages();
    // 加载历史消息
    try {
      const history = await invoke("session_history", { threadId: id });
      for (const h of history || []) {
        if (h.role === "user") {
          addMsg("user", renderMd(h.text));
        } else if (h.role === "assistant") {
          const b = addMsg("assistant", "");
          renderAssistantText(b, h.text);
        }
      }
    } catch (e2) { /* 历史加载失败不影响切换 */ }
    await loadSessions();
    toast("已切换到历史会话");
  } catch (e) {
    toast("切换会话失败：" + e, "err");
  }
}

async function deleteSession(id) {
  if (!confirm("确定删除这个会话吗？该操作不可恢复。")) return;
  try {
    const newTid = await invoke("delete_session", { threadId: id });
    if (newTid) {
      // 删除的是当前会话，已自动新建
      clearMessages();
      toast("会话已删除，已新建对话");
    } else {
      toast("会话已删除");
    }
    await loadSessions();
  } catch (e) {
    toast("删除会话失败：" + e, "err");
  }
}

async function switchWorkspace(path) {
  if (!path) return;
  if (path === state.workspace) { toast("已是当前工作区"); return; }
  const s = await invoke("get_settings").catch(() => null);
  if (!s) return;
  s.workspace_path = path;
  s.onboarded = true;
  try {
    await invoke("save_settings", { settings: s });
    const noAuth = s.no_auth || false;
    const hasKey = state.apiKey || (await invoke("has_api_key").catch(() => false));
    await refreshStatus();
    refreshFS();
    renderWorkspacePanel();
    // 切换/添加工作区后延迟再刷新一次文件树，确保引擎重启后仍显示新工作区文件
    setTimeout(refreshFS, 2500);
    if (hasKey || noAuth) {
      toast("已切换工作区，正在重启引擎…");
      startEngine();
      // 引擎重启后刷新会话列表
      setTimeout(loadSessions, 8000);
    } else {
      toast("工作区已切换");
    }
  } catch (e) {
    toast("切换失败：" + e, "err");
  }
}

async function pickFolderWithHint() {
  toast("请在弹出的窗口中选择一个文件夹（底部按钮为「选择文件夹」）");
  try {
    const picked = await invoke("pick_folder");
    return picked;
  } catch (e) {
    toast("选择文件夹失败：" + e, "err");
    return null;
  }
}

async function loadFS(path) {
  if (!state.workspace) return;
  const target = path || state.workspace;
  try {
    const entries = await invoke("list_dir", { path: target });
    state.fs.current = target;
    renderFS(entries);
  } catch (e) {
    state.fs.current = state.workspace;
    $("#fs-list").innerHTML = '<div class="fs-empty">' + escapeHtml(String(e)) + "</div>";
  }
}

function renderFS(entries) {
  const list = $("#fs-list");
  list.innerHTML = "";
  $("#fs-crumb").textContent = fsCrumbText(state.fs.current);
  $("#fs-crumb").title = state.fs.current;
  // 上级
  const ws = state.workspace.replace(/\\+$/, "");
  if (state.fs.current && state.fs.current !== ws && state.fs.current.length > ws.length) {
    const up = document.createElement("div");
    up.className = "fs-item fs-up";
    up.innerHTML = '<span class="fs-icon">⬆</span><span class="fs-name">上级目录…</span>';
    up.addEventListener("click", () => {
      const parent = state.fs.current.slice(0, Math.max(state.fs.current.lastIndexOf("\\"), state.fs.current.lastIndexOf("/")));
      loadFS(parent || ws);
    });
    list.appendChild(up);
  }
  if (!entries.length && !(state.fs.current && state.fs.current !== ws)) {
    const empty = document.createElement("div");
    empty.className = "fs-empty";
    empty.textContent = "（空目录）";
    list.appendChild(empty);
    return;
  }
  for (const e of entries) {
    const item = document.createElement("div");
    item.className = "fs-item" + (e.is_dir ? " fs-dir" : "");
    item.innerHTML =
      '<span class="fs-icon">' + (e.is_dir ? "📁" : "📄") + "</span>" +
      '<span class="fs-name">' + escapeHtml(e.name) + (e.is_dir ? "" : ' <span style="color:#8a94a3">' + fsFormatSize(e.size) + "</span>") + "</span>" +
      '<span class="fs-actions">' +
        (e.is_dir ? "" : '<button class="fs-btn fs-at" title="引用到对话（@）">@</button>') +
        '<button class="fs-btn" title="复制路径">⧉</button>' +
        '<button class="fs-btn" title="在文件夹中显示">📂</button>' +
      "</span>";
    const full = (state.fs.current.endsWith("\\") || state.fs.current.endsWith("/") ? state.fs.current : state.fs.current + "\\") + e.name;
    // 相对路径（供引用）
    const wsNorm = state.workspace.replace(/\\+$/, "");
    const rel = full.replace(/\\/g, "/").replace(new RegExp("^" + wsNorm.replace(/\\/g, "/").replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "/"), "");
    if (e.is_dir) {
      item.addEventListener("click", () => loadFS(full));
    } else {
      item.addEventListener("click", () => invoke("open_path", { path: full, reveal: false }).catch((err) => toast(String(err), "warn")));
    }
    const btns = item.querySelectorAll(".fs-btn");
    const atBtn = item.querySelector(".fs-at");
    if (atBtn) {
      atBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        addRef(rel);
        toast("已引用：" + rel);
      });
    }
    const copyBtn = item.querySelector('.fs-btn[title="复制路径"]');
    if (copyBtn) {
      copyBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        const copy = async () => {
          try { await navigator.clipboard.writeText(full); toast("路径已复制"); }
          catch (e2) { toast("复制失败：" + e2, "err"); }
        };
        copy();
      });
    }
    const revealBtn = item.querySelector('.fs-btn[title="在文件夹中显示"]');
    if (revealBtn) {
      revealBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        invoke("open_path", { path: full, reveal: true }).catch((err) => toast(String(err), "warn"));
      });
    }
    void btns;
    list.appendChild(item);
  }
}

async function refreshFS() {
  if (state.workspace) loadFS(state.fs.current || state.workspace);
}

// ---------- 设置 ----------
async function openSettings() {
  try {
    const s = await invoke("get_settings");
    state.settings = s;
    $("#set-provider").value = s.provider_name === "deepseek" ? "deepseek" : "custom";
    $("#set-base").value = s.base_url || "";
    $("#set-model").value = s.model || "";
    $("#set-winsandbox").value = s.windows_sandbox || "unelevated";
    $("#set-logdir").value = s.log_dir || "C:\\HARNESS\\logs";
    $("#set-noauth").checked = !!s.no_auth;
    $("#set-bridge").checked = !!s.use_bridge;
    $("#set-key").value = "";
    state.apiKey = "";
    updateWinSandboxTip();
    try {
      const hasKey = await invoke("has_api_key");
      $("#key-status").textContent = hasKey ? "（已保存，可自动启动引擎）" : "（未保存）";
    } catch (e) { /* ignore */ }
    // 显示当前实际使用的密钥（脱敏，前 6 + 后 4）
    try {
      const ki = await invoke("get_api_key_masked");
      $("#key-current").textContent = ki.present
        ? "当前使用：" + ki.masked + (ki.provider ? "（" + ki.provider + "）" : "")
        : "当前使用：未配置（请输入 Key 后保存）";
    } catch (e) { /* ignore */ }
    $("#modal-settings").classList.remove("hidden");
  } catch (e) {
    toast("读取设置失败：" + e, "err");
  }
}

function updateWinSandboxTip() {
  const v = $("#set-winsandbox").value;
  const tip = $("#winsandbox-tip");
  if (v === "unelevated") tip.textContent = "推荐：受限令牌沙箱，无需任何授权即可生效，足以把 Agent 限制在工作区内。";
  else tip.textContent = "增强沙箱：需要一次 UAC 授权，适合需要更精细读写控制的场景。";
}

// 测试模型连通性
async function testConnection() {
  const btn = $("#btn-test-conn");
  const res = $("#test-result");
  if (btn) btn.disabled = true;
  if (res) { res.className = "test-result"; res.textContent = "测试中…"; }
  try {
    // 优先用输入框里的 Key，否则用已保存的
    const key = state.apiKey || "";
    const r = await invoke("test_connection", { apiKey: key });
    if (res) {
      res.className = "test-result " + (r.ok ? "ok" : "fail");
      res.textContent = r.message;
    }
  } catch (e) {
    if (res) { res.className = "test-result fail"; res.textContent = "测试失败：" + e; }
  }
  if (btn) btn.disabled = false;
}

async function saveSettings() {
  const s = {
    workspace_path: (state.settings && state.settings.workspace_path) || "",
    provider_name: $("#set-provider").value === "deepseek" ? "deepseek" : "custom",
    base_url: $("#set-base").value.trim(),
    model: $("#set-model").value.trim(),
    api_key_env: "OH_API_KEY",
    sandbox_mode: (state.settings && state.settings.sandbox_mode) || "workspace-write",
    windows_sandbox: $("#set-winsandbox").value,
    log_dir: $("#set-logdir").value.trim() || "C:\\HARNESS\\logs",
    no_auth: $("#set-noauth").checked,
    use_bridge: $("#set-bridge").checked,
    onboarded: true,
  };
  try {
    await invoke("save_settings", { settings: s });
    // 仅当用户本次填写了 Key 才更新；留空则保留已保存的 Key（防止误清除）
    if (state.apiKey) {
      await invoke("save_api_key", { apiKey: state.apiKey });
    }
    state.settings = s;
    $("#modal-settings").classList.add("hidden");
    const noAuth = s.no_auth || false;
    const hasKey = state.apiKey || (await invoke("has_api_key").catch(() => false));
    state.running = false;
    setEngineChip("off", "");
    if (hasKey || noAuth) {
      toast("设置已保存，正在启动引擎…");
      startEngine();
    } else {
      toast("设置已保存（未填写 API Key 且未勾选内网免密钥，引擎未启动）");
    }
    await refreshStatus();
    refreshFS();
    renderWorkspacePanel();
    // 模式切换器同步当前沙箱
    renderMode(modeFromSandbox(s.sandbox_mode));
  } catch (e) {
    toast("保存失败：" + e, "err");
  }
}

// ---------- 文件引用（@） ----------
function fileBase(path) {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

function addRef(relPath) {
  if (!relPath) return;
  if (!state.refs.includes(relPath)) {
    state.refs.push(relPath);
  }
  renderRefs();
  closeMention();
}

function removeRef(relPath) {
  state.refs = state.refs.filter((r) => r !== relPath);
  renderRefs();
}

function renderRefs() {
  const bar = $("#refs-bar");
  bar.innerHTML = "";
  if (!state.refs.length) {
    bar.classList.add("hidden");
    $("#refs-clear").classList.add("hidden");
    return;
  }
  bar.classList.remove("hidden");
  $("#refs-clear").classList.remove("hidden");
  for (const r of state.refs) {
    const chip = document.createElement("span");
    chip.className = "ref-chip";
    chip.title = r;
    chip.innerHTML =
      '<span class="ref-icon">📎</span><span class="ref-name">' + escapeHtml(fileBase(r)) + "</span>" +
      '<button class="ref-del" title="移除引用">' + ICON_X + "</button>";
    chip.querySelector(".ref-del").addEventListener("click", () => removeRef(r));
    bar.appendChild(chip);
  }
}

async function ensureMentionFiles() {
  if (!state.mentionFiles.length && state.workspace) {
    try {
      state.mentionFiles = await invoke("list_workspace_files");
    } catch (e) {
      state.mentionFiles = [];
    }
  }
}

function onComposerInput() {
  const val = $("#composer").value;
  const caret = $("#composer").selectionStart ?? val.length;
  // 找光标前最近的 @（前面是开头或空白）
  const before = val.slice(0, caret);
  const atIdx = before.lastIndexOf("@");
  if (atIdx >= 0) {
    const pre = atIdx === 0 ? "" : before[atIdx - 1];
    if (pre === "" || /\s/.test(pre)) {
      const query = before.slice(atIdx + 1).toLowerCase();
      state.mentionQuery = query;
      openMention(query);
      return;
    }
  }
  closeMention();
}

async function openMention(query) {
  await ensureMentionFiles();
  const panel = $("#mention-panel");
  const list = $("#mention-list");
  panel.classList.remove("hidden");
  const q = (query || "").toLowerCase();
  const filtered = state.mentionFiles.filter((f) => !q || f.toLowerCase().includes(q));
  list.innerHTML = "";
  const empty = $("#mention-empty");
  if (!filtered.length) {
    empty.classList.remove("hidden");
    list.classList.add("hidden");
  } else {
    empty.classList.add("hidden");
    list.classList.remove("hidden");
    for (const f of filtered.slice(0, 60)) {
      const item = document.createElement("div");
      item.className = "mention-item";
      item.innerHTML = '<span class="mention-icon">📄</span><span class="mention-name">' + escapeHtml(f) + "</span>";
      item.addEventListener("click", () => {
        addRef(f);
        stripAtQuery();
        $("#composer").focus();
      });
      list.appendChild(item);
    }
  }
}

function closeMention() {
  $("#mention-panel").classList.add("hidden");
}

function stripAtQuery() {
  // 移除输入框中未完成的 @query 文本
  const ta = $("#composer");
  const val = ta.value;
  const caret = ta.selectionStart ?? val.length;
  const before = val.slice(0, caret);
  const atIdx = before.lastIndexOf("@");
  if (atIdx >= 0) {
    const pre = atIdx === 0 ? "" : before[atIdx - 1];
    if (pre === "" || /\s/.test(pre)) {
      const newVal = val.slice(0, atIdx) + val.slice(caret);
      ta.value = newVal;
      const pos = atIdx;
      ta.setSelectionRange(pos, pos);
      return;
    }
  }
}

function buildMessageWithRefs(text) {
  if (!state.refs.length) return text;
  const lines = state.refs.map((r) => "- " + r).join("\n");
  return text + "\n\n（本任务引用的工作区文件：\n" + lines + "\n请基于这些文件执行。）";
}

// ---------- 发送 ----------
async function send() {
  const text = $("#composer").value;
  if (!text.trim() && !state.refs.length) return;
  // 引擎未就绪时给出明确提示，而不是静默无响应
  if (!state.running) {
    toast("引擎未在运行，正在尝试自动启动…", "warn");
    const started = await startEngine();
    if (!started) return;
  }
  closeMention();
  const full = buildMessageWithRefs(text.trim());
  $("#composer").value = "";
  const userHtml = text.trim() ? renderMd(text.trim()) : "";
  const refsHtml = state.refs.length
    ? '<div class="tag-block tag-plan"><div class="tag-title">📎 引用文件</div>' +
      state.refs.map((r) => escapeHtml("· " + r)).join("<br>") + "</div>"
    : "";
  addMsg("user", userHtml + refsHtml);
  const btn = $("#btn-send");
  btn.disabled = true;
  btn.textContent = "发送中…";
  showRunBar("任务运行中…", null);
  try {
    await invoke("send_message", { text: full });
  } catch (e) {
    // 安全拒绝或引擎错误 → 红色警告
    addMsg("assistant", '<div class="tag-block tag-warn"><div class="tag-title">⛔ 已拦截</div>' + escapeHtml(String(e)) + "</div>");
    if (!handleEngineError(String(e))) toast("任务未执行：" + String(e), "warn");
    hideRunBar();
  } finally {
    btn.disabled = state.running ? false : true;
    btn.textContent = "发送";
  }
}

// ---------- 启动/停止 ----------
async function startEngine() {
  const key = state.apiKey;
  // 先查后端真实状态，避免「显示未启动但实际运行中」的矛盾
  try {
    const st = await invoke("get_status");
    if (st.running) {
      state.running = true;
      setEngineChip("running", "");
      toast("引擎已在运行中");
      return true;
    }
  } catch (e) { /* 查询失败继续尝试启动 */ }
  try {
    setEngineChip("starting", "");
    await invoke("start_engine", { apiKey: key || "" });
    state.running = true;
    setEngineChip("running", "");
    toast("引擎已启动");
    setupMemoryCard(); // 记忆服务随引擎启动，加载内嵌面板
    return true;
  } catch (e) {
    state.running = false;
    setEngineChip("off", "");
    // 若提示缺 Key，引导打开设置
    const msg = String(e);
    toast("启动失败：" + msg, "err");
    if (msg.includes("API Key") || msg.includes("api key") || msg.includes("401") || msg.includes("invalid")) {
      openSettings();
    }
    return false;
  }
}

async function stopEngine() {
  try {
    await invoke("stop_engine");
    state.running = false;
    setEngineChip("off", "");
    toast("引擎已停止");
  } catch (e) {
    toast("停止失败：" + e, "err");
  }
}

// ---------- 初始化 ----------
async function init() {
  // 绑定事件
  $("#btn-settings").addEventListener("click", openSettings);
  $("#btn-settings-cancel").addEventListener("click", () => $("#modal-settings").classList.add("hidden"));
  $("#btn-settings-save").addEventListener("click", saveSettings);
  $("#btn-onboard-go").addEventListener("click", () => {
    $("#modal-onboard").classList.add("hidden");
    openSettings(); // 引导完成后直接进入设置，填写 API Key
  });
  $("#btn-ap-allow").addEventListener("click", () => resolveApproval("accept"));
  $("#btn-ap-deny").addEventListener("click", () => resolveApproval("decline"));
  $("#btn-test-conn").addEventListener("click", testConnection);
  $("#btn-send").addEventListener("click", send);
  $("#btn-interrupt").addEventListener("click", async () => {
    try { await invoke("interrupt"); } catch (e) { /* ignore */ }
  });
  // 权限模式切换
  document.querySelectorAll(".mode-option").forEach((opt) => {
    opt.addEventListener("click", () => switchMode(opt.dataset.mode));
  });
  // 初始化模式（读当前沙箱设置）
  const initS = await invoke("get_settings").catch(() => null);
  if (initS) {
    state.settings = initS;
    renderMode(modeFromSandbox(initS.sandbox_mode || "workspace-write"));
  }
  $("#btn-add-ws").addEventListener("click", async () => {
    const picked = await pickFolderWithHint();
    if (picked) switchWorkspace(picked);
  });
  $("#btn-new-session").addEventListener("click", newSession);
  $("#btn-fs-refresh").addEventListener("click", refreshFS);

  // SKILLS：标题点击折叠/展开
  $("#skills-head").addEventListener("click", () => {
    const body = $("#skills-body");
    const toggle = $("#skills-toggle");
    if (body.classList.contains("hidden")) {
      body.classList.remove("hidden");
      toggle.textContent = "▾ SKILLS 仓库";
      renderSkills();
    } else {
      body.classList.add("hidden");
      toggle.textContent = "▸ SKILLS 仓库";
    }
  });
  // SKILLS：＋ 添加菜单（创建自定义 / 导入）
  $("#btn-skills-add").addEventListener("click", (e) => {
    e.stopPropagation();
    const menu = $("#skills-add-menu");
    menu.classList.toggle("hidden");
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#skills-add-menu") && !e.target.closest("#btn-skills-add")) {
      $("#skills-add-menu").classList.add("hidden");
    }
  });
  document.querySelectorAll(".skills-add-item").forEach((item) => {
    item.addEventListener("click", () => {
      $("#skills-add-menu").classList.add("hidden");
      if (item.dataset.action === "create") {
        // 引导用自然语言描述技能
        const ta = $("#composer");
        ta.focus();
        ta.placeholder = "请用自然语言描述要创建的技能，例如：创建一个技能，把 Excel 按月份汇总并生成图表。描述后直接发送…";
        toast("请用自然语言描述要创建的技能，然后发送", "warn");
      } else {
        $("#modal-import-skill").classList.remove("hidden");
      }
    });
  });
  $("#btn-import-cancel").addEventListener("click", () => $("#modal-import-skill").classList.add("hidden"));
  $("#btn-import-pick").addEventListener("click", async () => {
    const picked = await pickFolderWithHint();
    if (!picked) return;
    toast("正在导入技能…");
    try {
      const imported = await invoke("import_skills", { dir: picked });
      if (imported && imported.length) {
        toast("已导入 " + imported.length + " 个技能：" + imported.join("、"));
      } else {
        toast("所选目录中未找到含 SKILL.md 的技能文件夹", "warn");
      }
    } catch (err) {
      toast("导入失败：" + err, "err");
    }
    $("#modal-import-skill").classList.add("hidden");
    renderSkills();
  });

  $("#set-winsandbox").addEventListener("change", updateWinSandboxTip);
  $("#set-key").addEventListener("input", (e) => { state.apiKey = e.target.value.trim(); });

  // 引用清除
  $("#refs-clear").addEventListener("click", () => { state.refs = []; renderRefs(); });

  // 内嵌记忆面板：打开按钮 / 重试 / iframe 阶段确认与尺寸联动
  $("#btn-mem-open").addEventListener("click", openMemoryPanel);
  $("#btn-mem-retry").addEventListener("click", setupMemoryCard);
  window.addEventListener("message", (ev) => {
    if (ev.origin !== MEMORY_ORIGIN) return;
    const d = ev.data || {};
    if (d.type === "memory:phase-confirmed") {
      toast("阶段已归档" + (d.openNewThread ? "，正在开启新阶段对话…" : ""), "ok");
      if (d.openNewThread) startPhaseThread(d.summary || d.goal || "");
    } else if (d.type === "memory:size") {
      // 面板汇报记忆块数量 → 自适应高度（空=文件窗口高，有块=最多 3 张卡）
      if (typeof d.count === "number") {
        state.memoryCount = d.count;
        updateMemoryFrameHeight();
      }
    }
  });

  // 输入 @ 触发文件引用面板
  $("#composer").addEventListener("input", onComposerInput);
  $("#composer").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
    if (e.key === "Escape") closeMention();
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#mention-panel") && !e.target.closest("#composer")) {
      closeMention();
    }
  });

  await initEvents();
  await refreshStatus();
  if (state.workspace) refreshFS();
  await renderWorkspacePanel();
  await renderSkills();
  if (state.running) loadSessions();
  setupMemoryCard();
  updateChatEmptyBg();
  updateMemoryFrameHeight();
  // 记忆块数量变化 → 面板高度自适应（兜底轮询，避免遗漏 iframe 消息）
  setInterval(refreshMemoryCount, 20000);

  // 轮询引擎状态：无条件以后端为准同步 UI（兜底自动启动竞态与事件丢失）
  setInterval(async () => {
    try {
      const s = await invoke("get_status");
      if (s.running !== state.running) {
        state.running = s.running;
        setEngineChip(s.running ? "running" : "off", "");
        if (s.running) {
          loadSessions();
          if (state.workspace) refreshFS();
        }
      }
    } catch (e) { /* ignore */ }
  }, 1500);

  // 若已配置且引擎未启动（例如自动启动未成功），在设置页给出提示即可，不打扰主界面
  setTimeout(async () => {
    if (!state.running && state.settings && state.settings.onboarded) {
      const s = await invoke("get_settings").catch(() => null);
      if (s && s.onboarded) {
        toast("引擎未在运行：打开右上角「设置」可手动启动，或检查 API Key", "warn");
      }
    }
  }, 5000);

  // 首次使用引导
  try {
    const s = await invoke("get_settings");
    if (!s.onboarded) $("#modal-onboard").classList.remove("hidden");
  } catch (e) { /* ignore */ }
}

window.addEventListener("DOMContentLoaded", () => {
  window.addEventListener("error", (e) => {
    showFatalError("JS 错误：" + (e.message || e.type));
  });
  init().catch((err) => {
    console.error("[init error]", err);
    showFatalError("初始化失败：" + (err && err.message ? err.message : err));
  });
});
