/* HARNESS 记忆面板 —— 交互逻辑（原生 JS，无框架） */
"use strict";

// 由记忆服务托管时用相对路径；file:// 直开时用绝对地址（演示模式）
const API_BASE = location.protocol === "file:" ? "http://127.0.0.1:8765/api" : "/api";
// 本地会话令牌：主应用以 ?token= 注入 iframe URL（R2 鉴权）
const TOKEN = new URLSearchParams(location.search).get("token") || "";
const SKELETON_DELAY = 2000;   // API 响应超 2 秒显示骨架屏
const POLL_MS = 30000;         // 水位轮询

// 是否嵌入主应用侧栏（iframe）
const EMBEDDED = (() => { try { return window.self !== window.top; } catch (e) { return false; } })();

const TYPE_LABEL = {
  fact: "事实", preference: "偏好", task: "任务", code_snippet: "代码",
  plan: "计划", constraint: "约束", user_defined: "自定义", phase: "阶段",
};
const STATUS_LABEL = { active: "启用", paused: "暂停", probation: "观察", deprecated: "已归档" };

/* ---------- 演示数据（API 不可用时展示，双击 HTML 即可看静态样机） ---------- */
const MOCK_BLOCKS = [
  { id: "mem_mock_1", type: "constraint", content: "所有文件操作必须限制在工作区内", importance: 5, status: "active", token_count: 14, last_accessed: "2026-08-25T10:00:00", source_round: 1, is_pinned: true, order_index: 0 },
  { id: "mem_mock_2", type: "fact", content: "销售数据文件位于工作区根目录", importance: 4, status: "active", token_count: 11, last_accessed: "2026-08-25T10:00:00", source_round: 1, is_pinned: false, order_index: 1 },
  { id: "mem_mock_3", type: "code_snippet", content: "openpyxl 读取 xlsx 的脚本位于 scripts/merge.py", importance: 4, status: "active", token_count: 13, last_accessed: "2026-08-25T10:00:00", source_round: 2, is_pinned: false, order_index: 2 },
  { id: "mem_mock_4", type: "plan", content: "下一步：生成 2025 年各季度同比分析", importance: 3, status: "active", token_count: 10, last_accessed: "2026-08-25T10:00:00", source_round: 3, is_pinned: false, order_index: 3 },
  { id: "mem_mock_5", type: "preference", content: "用户偏好：汇总表导出为 xlsx 格式", importance: 3, status: "paused", token_count: 12, last_accessed: "2026-08-25T10:00:00", source_round: 4, is_pinned: false, order_index: 4 },
];
const MOCK_STATUS = {
  conversation_tokens: 48500, round: 18, level: "warn",
  needs_compact: true, over_limit: false, needs_handoff: false,
  pool_tokens: 60, active_count: 4, total_count: 5,
  thresholds: { compaction: 52000, max: 60000 },
  urgency: "上下文进入警戒区，建议压缩记忆。",
};

/* ---------- 全局状态 ---------- */
const state = {
  blocks: [],
  status: null,
  mock: false,
  editingId: null,
  dragId: null,
  handoffMarkdown: "",
};

const $ = (s) => document.querySelector(s);

/* ================= Toast ================= */
let toastTimer = null;
function toast(msg, type) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "toast" + (type === "ok" ? " ok" : "");
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 3000);
}

/* ================= API（不可用时自动降级 Mock） ================= */
async function api(path, opts) {
  const headers = { "Content-Type": "application/json" };
  if (TOKEN) headers["Authorization"] = "Bearer " + TOKEN;  // R2 本地令牌鉴权
  const res = await fetch(API_BASE + path, {
    headers,
    ...opts,
  });
  if (!res.ok) throw new Error("HTTP " + res.status);
  return res.json();
}

async function tryApi(path, opts) {
  if (state.mock) throw new Error("mock");
  try {
    return await api(path, opts);
  } catch (e) {
    state.mock = true;
    throw e;
  }
}

/* ================= 主题（明/暗） ================= */
function applyTheme(theme) {
  document.body.dataset.theme = theme;
  $("#theme-toggle").textContent = theme === "dark" ? "☀️" : "🌙";
  localStorage.setItem("hm-theme", theme);
}
function initTheme() {
  const saved = localStorage.getItem("hm-theme");
  const prefersDark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  applyTheme(saved || (prefersDark ? "dark" : "light"));
  if (EMBEDDED) document.body.classList.add("compact");
}

/* ================= 操作模式（简单 / 专业） ================= */
function applyMode(mode) {
  document.body.classList.toggle("simple", mode === "simple");
  $("#mode-simple").classList.toggle("on", mode === "simple");
  $("#mode-pro").classList.toggle("on", mode === "pro");
  localStorage.setItem("hm-mode", mode);
}
function initMode() {
  // 嵌入侧栏：强制极简模式（无头部切换入口），独立打开才用「简单/专业」选择
  if (EMBEDDED) {
    applyMode("simple");
    return;
  }
  const saved = localStorage.getItem("hm-mode");
  applyMode(saved || "pro");
}

/* ================= 头部渲染 ================= */
function renderHeader() {
  const st = state.status || MOCK_STATUS;
  const max = st.thresholds.max || 60000;
  const warnAt = st.thresholds.compaction || 52000;
  const tokens = st.conversation_tokens || 0;
  const bar = $("#token-bar");
  const pct = Math.min(100, (tokens / max) * 100);
  bar.style.width = pct + "%";
  bar.className = "progress-fill " + (tokens >= max ? "critical" : tokens >= warnAt ? "warn" : "");
  $("#token-text").textContent =
    "上下文 " + fmtK(tokens) + " / " + fmtK(max) + " tokens";
  const lv = $("#watermark-level");
  const lvMap = {
    ok: ["正常", "wm-ok"], warn: ["警戒区", "wm-warn"], critical: ["超红线", "wm-critical"],
  };
  const [txt, cls] = lvMap[st.level] || lvMap.ok;
  lv.textContent = txt;
  lv.className = cls;
  $("#block-count").textContent = (st.total_count ?? state.blocks.length) + " 个记忆块";
  $("#pool-tokens").textContent = "记忆池 " + fmtK(st.pool_tokens || 0) + " tok";
  const badge = $("#svc-badge");
  if (state.mock) {
    badge.textContent = "演示数据";
    badge.className = "svc-badge mock";
  } else {
    badge.textContent = "已连接";
    badge.className = "svc-badge";
  }
  // 上下文水位恢复后，允许红线自动总结再次触发
  if (!st.over_limit) state.redlineAuto = false;
}

function fmtK(n) {
  n = n || 0;
  if (n >= 1000) return (n / 1000).toFixed(1) + "k";
  return String(n);
}

/* ================= 卡片渲染 ================= */
function stars(imp) {
  let s = "";
  for (let i = 1; i <= 5; i++) s += i <= imp ? "★" : '<span class="empty">★</span>';
  return '<span class="stars">' + s + "</span>";
}

function renderCards() {
  const list = $("#memory-list");
  const blocks = [...state.blocks].sort((a, b) => (a.order_index || 0) - (b.order_index || 0));
  if (!blocks.length) {
    list.innerHTML = '<div class="empty-tip">还没有记忆块。<br>完成任务后可点击「阶段总结」归档本阶段关键信息。</div>';
    return;
  }
  list.innerHTML = "";
  for (const b of blocks) {
    const card = document.createElement("div");
    card.className = "card";
    card.dataset.id = b.id;
    card.draggable = true;
    if (b.status === "paused") card.classList.add("paused");
    if (b.status === "probation") card.classList.add("probation");

    const row = document.createElement("div");
    row.className = "card-row";

    const handle = document.createElement("span");
    handle.className = "drag-handle";
    handle.textContent = "⋮⋮";

    const tag = document.createElement("span");
    tag.className = "tag tg-" + b.type;
    tag.textContent = TYPE_LABEL[b.type] || b.type;

    const actions = document.createElement("div");
    actions.className = "card-actions";
    actions.appendChild(miniBtn("🔒", () => togglePin(b), "置顶/取消置顶"));
    actions.appendChild(miniBtn(b.status === "paused" ? "▶️" : "⏸️", () => toggleBlock(b), "启用/暂停"));
    actions.appendChild(miniBtn("✏️", () => openEdit(b), "编辑"));
    actions.appendChild(miniBtn("🗑", () => delBlock(b), "删除", true));

    row.append(handle, tag, actions);

    const content = document.createElement("div");
    content.className = "card-content";
    content.textContent = b.content;

    const meta = document.createElement("div");
    meta.className = "card-meta";
    const st = document.createElement("span");
    st.className = "status-badge";
    st.textContent = STATUS_LABEL[b.status] || b.status;
    const info = document.createElement("span");
    info.textContent = "t" + (b.token_count || 0) + " · 轮 " + (b.source_round ?? 0);
    meta.append(stars(b.importance || 3), st, info);
    if (b.is_pinned) {
      const pin = document.createElement("span");
      pin.className = "pin";
      pin.textContent = "📌 置顶";
      meta.appendChild(pin);
    }

    card.append(row, content, meta);
    list.appendChild(card);
  }
  bindDrag();
  reportSize();
}

// 向主应用汇报记忆块数量（用于面板高度自适应：空=文件窗口高，有块=最多 3 张卡）
function reportSize() {
  if (!EMBEDDED) return;
  try {
    parent.postMessage({ type: "memory:size", count: state.blocks.length }, "*");
  } catch (e) { /* ignore */ }
}

function miniBtn(label, fn, title, danger) {
  const b = document.createElement("button");
  b.className = "mini-btn" + (danger ? " del" : "");
  b.textContent = label;
  b.title = title || "";
  b.addEventListener("click", (e) => { e.stopPropagation(); fn(); });
  return b;
}

/* ================= 骨架屏 ================= */
function showSkeleton() {
  const list = $("#memory-list");
  list.innerHTML = Array.from({ length: 4 }, () => '<div class="skel-card"></div>').join("");
}

/* ================= 数据加载 ================= */
async function loadAll() {
  const list = $("#memory-list");
  const skelTimer = setTimeout(showSkeleton, SKELETON_DELAY);
  try {
    const [blk, st] = await Promise.all([
      tryApi("/memory/blocks"),
      tryApi("/memory/status"),
    ]);
    state.blocks = blk.blocks || [];
    state.status = st;
    state.mock = false;
  } catch (e) {
    // 后端不可用 → 演示数据（Mock 样机）
    state.mock = true;
    state.blocks = MOCK_BLOCKS.map((m) => ({ ...m }));
    state.status = MOCK_STATUS;
  } finally {
    clearTimeout(skelTimer);
  }
  renderHeader();
  renderCards();
  maybeAutoPhase();   // 启动即检查：若上下文已超红线则自动生成阶段总结
  pollStatus();
}

function pollStatus() {
  setTimeout(async () => {
    try {
      const st = await tryApi("/memory/status");
      state.status = st;
      renderHeader();
      maybeAutoPhase();
    } catch (e) { /* 保持离线 */ }
    pollStatus();
  }, POLL_MS);
}

/* ================= 自动总结（上下文达红线时自动触发） ================= */
async function maybeAutoPhase() {
  const st = state.status;
  if (!st || state.mock) return;
  if (!st.over_limit || state.redlineAuto || state.phaseBusy) return;
  state.redlineAuto = true;   // 防重复弹窗；水位恢复后自动复位
  state.phaseBusy = true;
  state.phaseAuto = true;
  // 立即打开弹窗（生成中状态），避免常驻横幅「一直转」
  const goal = "上下文已满，自动总结当前阶段";
  $("#phase-goal").value = goal;
  $("#phase-preview").hidden = false;
  $("#phase-preview").textContent = "正在生成阶段总结…";
  $("#phase-new-thread").checked = true;   // 自动开启新阶段对话，真正释放上下文
  $("#phase-auto-note").hidden = false;
  $("#phase-confirm").disabled = true;
  $("#phase-modal").hidden = false;
  try {
    const r = await tryApi("/memory/phase/preview", { method: "POST", body: JSON.stringify({ goal }) });
    state.phaseSummary = r.summary || "";
    const pv = $("#phase-preview");
    pv.textContent = state.phaseSummary +
      (r.tokens != null ? "\n\n（约 " + r.tokens + " tokens · 基于 " + (r.blocks_used || 0) + " 个记忆块）" : "");
    $("#phase-confirm").disabled = false;
  } catch (e) {
    state.phaseAuto = false;
    $("#phase-modal").hidden = true;
    toast("自动生成阶段总结失败：" + e, "err");
  } finally {
    state.phaseBusy = false;
  }
}

/* ================= 卡片操作 ================= */
async function toggleBlock(b) {
  try {
    const target = b.status === "paused" ? "active" : "paused";
    const r = await tryApi("/memory/blocks/" + b.id, {
      method: "PATCH", body: JSON.stringify({ status: target }),
    });
    const nb = r.block;
    const i = state.blocks.findIndex((x) => x.id === b.id);
    if (i >= 0) state.blocks[i] = nb;
    renderCards();
    toast(nb.status === "paused" ? "已暂停（不再进入上下文）" : "已启用", "ok");
  } catch (e) {
    toast("操作失败：服务未连接（演示模式）");
  }
}

async function togglePin(b) {
  try {
    const r = await tryApi("/memory/blocks/" + b.id, {
      method: "PATCH", body: JSON.stringify({ is_pinned: !b.is_pinned }),
    });
    const nb = r.block;
    const i = state.blocks.findIndex((x) => x.id === b.id);
    if (i >= 0) state.blocks[i] = nb;
    renderCards();
  } catch (e) {
    toast("操作失败：服务未连接（演示模式）");
  }
}

async function delBlock(b) {
  if (!confirm("删除记忆块：\n" + b.content + "\n\n确定删除？")) return;
  try {
    await tryApi("/memory/blocks/" + b.id, { method: "DELETE" });
    state.blocks = state.blocks.filter((x) => x.id !== b.id);
    renderCards();
    toast("已删除", "ok");
  } catch (e) {
    toast("删除失败：服务未连接（演示模式）");
  }
}

/* ================= 手动添加 / 编辑 ================= */
function openAdd() {
  state.editingId = null;
  $("#add-modal-title").textContent = "添加记忆块";
  $("#add-type").value = "fact";
  $("#add-content").value = "";
  $("#add-importance").value = "3";
  $("#add-modal").hidden = false;
  setTimeout(() => $("#add-content").focus(), 50);
}

function openEdit(b) {
  state.editingId = b.id;
  $("#add-modal-title").textContent = "编辑记忆块";
  $("#add-type").value = b.type;
  $("#add-content").value = b.content;
  $("#add-importance").value = String(b.importance || 3);
  $("#add-modal").hidden = false;
  setTimeout(() => $("#add-content").focus(), 50);
}

async function saveBlock() {
  const type = $("#add-type").value;
  const content = $("#add-content").value.trim();
  const importance = parseInt($("#add-importance").value, 10) || 3;
  if (!content) { toast("内容不能为空"); return; }
  try {
    if (state.editingId) {
      await tryApi("/memory/blocks/" + state.editingId, {
        method: "PATCH", body: JSON.stringify({ content, type, importance }),
      });
      toast("已保存", "ok");
    } else {
      await tryApi("/memory/blocks", {
        method: "POST", body: JSON.stringify({ type, content, importance }),
      });
      toast("已添加记忆块", "ok");
    }
    $("#add-modal").hidden = true;
    await refreshBlocks();
  } catch (e) {
    toast("保存失败：服务未连接（演示模式）");
  }
}

/* ================= 拖拽排序 ================= */
function bindDrag() {
  const list = $("#memory-list");
  let overId = null;
  list.querySelectorAll(".card").forEach((card) => {
    card.addEventListener("dragstart", () => {
      state.dragId = card.dataset.id;
      card.classList.add("dragging");
    });
    card.addEventListener("dragend", async () => {
      card.classList.remove("dragging");
      if (state.dragId) {
        const ids = [...list.querySelectorAll(".card")].map((c) => c.dataset.id);
        const items = ids.map((id, idx) => ({ id, order_index: idx }));
        state.blocks.forEach((b) => {
          const p = ids.indexOf(b.id);
          if (p >= 0) b.order_index = p;
        });
        renderCards();
        try {
          await tryApi("/memory/blocks/order", {
            method: "PATCH", body: JSON.stringify({ items }),
          });
        } catch (e) { /* 离线 */ }
        state.dragId = null;
      }
    });
    card.addEventListener("dragover", (e) => {
      e.preventDefault();
      const target = card.dataset.id;
      if (target && target !== state.dragId && target !== overId) {
        overId = target;
        const after = e.clientY > card.getBoundingClientRect().top + card.offsetHeight / 2;
        if (after) card.after(document.querySelector(`.card[data-id="${state.dragId}"]`));
        else card.before(document.querySelector(`.card[data-id="${state.dragId}"]`));
      }
    });
  });
}

/* ================= 交接流程 ================= */
async function openHandoff() {
  try {
    toast("正在生成交接文档…", "ok");
    const r = await tryApi("/memory/handoff/preview", { method: "POST", body: "{}" });
    state.handoffMarkdown = r.markdown || "";
    $("#handoff-meta").textContent =
      "约 " + (r.tokens || 0) + " tokens · " + (r.chars || 0) + " 字" +
      (r.needs ? " · 已满足交接触发条件" : "");
    $("#handoff-body").textContent = state.handoffMarkdown;
    $("#handoff-modal").hidden = false;
  } catch (e) {
    toast("服务未连接，无法生成交接（演示模式）");
  }
}

async function confirmHandoff() {
  if (!confirm("确认交接？旧记忆将暂存 30 分钟（可紧急回滚），记忆池重置为 3 个种子块。")) return;
  try {
    const r = await tryApi("/memory/handoff/confirm", { method: "POST", body: "{}" });
    $("#handoff-modal").hidden = true;
    await refreshBlocks();
    toast(r.message || "交接完成", "ok");
  } catch (e) {
    toast("交接失败：服务未连接（演示模式）");
  }
}

async function rollbackHandoff() {
  if (!confirm("紧急回滚：恢复最近一次记忆快照（30 分钟窗口内）？")) return;
  try {
    const r = await tryApi("/memory/handoff/rollback", { method: "POST", body: "{}" });
    await refreshBlocks();
    toast(r.message || "已回滚", "ok");
  } catch (e) {
    toast("回滚失败：服务未连接（演示模式）");
  }
}

async function refreshBlocks() {
  try {
    const blk = await tryApi("/memory/blocks");
    state.blocks = blk.blocks || [];
    const st = await tryApi("/memory/status");
    state.status = st;
  } catch (e) { /* 保持现状 */ }
  renderHeader();
  renderCards();
}

/* ================= 事件绑定 ================= */
function bindEvents() {
  $("#theme-toggle").addEventListener("click", () => {
    applyTheme(document.body.dataset.theme === "dark" ? "light" : "dark");
  });
  $("#mode-simple").addEventListener("click", () => applyMode("simple"));
  $("#mode-pro").addEventListener("click", () => applyMode("pro"));
  $("#btn-add").addEventListener("click", openAdd);
  $("#add-cancel").addEventListener("click", () => { $("#add-modal").hidden = true; });
  $("#add-save").addEventListener("click", saveBlock);
  $("#add-content").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) saveBlock();
  });
  $("#btn-handoff").addEventListener("click", openHandoff);
  // 阶段总结
  $("#btn-phase").addEventListener("click", openPhaseModal);
  $("#phase-close").addEventListener("click", closePhaseModal);
  $("#phase-cancel").addEventListener("click", closePhaseModal);
  $("#phase-gen").addEventListener("click", generatePhase);
  $("#phase-confirm").addEventListener("click", confirmPhase);
  $("#phase-history").addEventListener("click", openPhasesHistory);
  $("#phases-close").addEventListener("click", () => { $("#phases-modal").hidden = true; });
  $("#handoff-close").addEventListener("click", () => { $("#handoff-modal").hidden = true; });
  $("#handoff-copy").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(state.handoffMarkdown);
      toast("已复制到剪贴板", "ok");
    } catch (e) {
      toast("复制失败");
    }
  });
  $("#handoff-confirm").addEventListener("click", confirmHandoff);
  $("#handoff-rollback").addEventListener("click", rollbackHandoff);
  // 点击弹窗遮罩关闭
  document.querySelectorAll(".modal").forEach((m) => {
    m.addEventListener("click", (e) => {
      if (e.target === m) m.hidden = true;
    });
  });
}

/* ================= 阶段总结流程（按阶段工作） ================= */
function escapeHtml(s) {
  return String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

function closePhaseModal() {
  $("#phase-modal").hidden = true;
  if (state.phaseAuto) {
    // 自动触发的总结被关闭：本轮红线不再重复弹窗；水位恢复后自动复位
    state.redlineAuto = true;
    toast("已关闭。需要时仍可手动点击「阶段总结」。");
  }
}

async function openPhaseModal() {
  state.phaseAuto = false;
  $("#phase-goal").value = "";
  $("#phase-preview").hidden = true;
  $("#phase-preview").textContent = "";
  $("#phase-confirm").disabled = true;
  $("#phase-new-thread").checked = false;
  $("#phase-auto-note").hidden = true;
  $("#phase-modal").hidden = false;
  setTimeout(() => $("#phase-goal").focus(), 50);
}

async function generatePhase() {
  const goal = $("#phase-goal").value.trim();
  const btn = $("#phase-gen");
  btn.disabled = true;
  btn.textContent = "生成中…";
  try {
    const r = await tryApi("/memory/phase/preview", { method: "POST", body: JSON.stringify({ goal }) });
    state.phaseSummary = r.summary || "";
    const pv = $("#phase-preview");
    pv.hidden = false;
    pv.textContent = state.phaseSummary +
      (r.tokens != null ? "\n\n（约 " + r.tokens + " tokens · 基于 " + (r.blocks_used || 0) + " 个记忆块）" : "");
    $("#phase-confirm").disabled = false;
    toast("总结已生成，确认后旧记忆块将自动归档", "ok");
  } catch (e) {
    toast("生成失败：服务未连接（演示模式）");
  } finally {
    btn.disabled = false;
    btn.textContent = "✨ 生成总结";
  }
}

async function confirmPhase() {
  const goal = $("#phase-goal").value.trim();
  const openNew = $("#phase-new-thread").checked;
  if (!state.phaseSummary) { toast("请先生成总结"); return; }
  try {
    const r = await tryApi("/memory/phase/confirm", {
      method: "POST",
      body: JSON.stringify({ goal, summary: state.phaseSummary, open_new_thread: openNew }),
    });
    $("#phase-modal").hidden = true;
    state.phaseAuto = false;
    // 归档后不再自动弹窗；若开启了新阶段对话，水位复位后自动恢复下一轮触发
    state.redlineAuto = true;
    await refreshBlocks();
    toast(r.message || "阶段已归档", "ok");
    // 通知主应用：按需开启新阶段对话（首条消息 = 阶段总结，释放上下文）
    if (EMBEDDED) {
      parent.postMessage({
        type: "memory:phase-confirmed",
        summary: state.phaseSummary,
        goal,
        openNewThread: openNew,
      }, "*");
    } else if (openNew) {
      toast("已归档。在主应用侧栏确认后即可开启新阶段对话。");
    }
  } catch (e) {
    toast("归档失败：服务未连接（演示模式）");
  }
}

async function openPhasesHistory() {
  try {
    const r = await tryApi("/memory/phases");
    const ps = r.phases || [];
    const list = $("#phases-list");
    if (!ps.length) {
      list.innerHTML = '<div class="phase-empty">暂无阶段记录</div>';
    } else {
      list.innerHTML = "";
      for (const p of ps.slice().reverse()) {
        const el = document.createElement("div");
        el.className = "phase-item";
        const date = (p.created_at || "").replace("T", " ").slice(0, 16);
        el.innerHTML =
          '<div class="ph-date">' + date + " · 归档 " + (p.archived || 0) + " 块</div>" +
          '<div class="ph-goal">' + escapeHtml(p.goal || "(未填写阶段说明)") + "</div>" +
          '<div class="ph-summary">' + escapeHtml(p.summary || "") + "</div>";
        list.appendChild(el);
      }
    }
    $("#phases-modal").hidden = false;
  } catch (e) {
    toast("读取失败：服务未连接（演示模式）");
  }
}

/* ================= 启动 ================= */
initTheme();
initMode();
bindEvents();
loadAll();
