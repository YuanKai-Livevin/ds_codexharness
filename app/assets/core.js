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
function fsFormatSize(n) {
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(1) + " GB";
  if (n >= 1 << 20) return (n / (1 << 20)).toFixed(1) + " MB";
  if (n >= 1 << 10) return (n / (1 << 10)).toFixed(0) + " KB";
  return n + " B";
}

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
