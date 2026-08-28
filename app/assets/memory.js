async function openMemoryPanel() {
  try {
    const st = await invoke("memory_status").catch(() => null);
    if (st && !st.running) {
      toast("记忆服务未运行，无法打开面板。");
      return;
    }
    await invoke("open_memory_panel");
  } catch (e) {
    toast("打开记忆面板失败：" + e, "err");
  }
}

// ---------- 内嵌记忆面板（主应用直接渲染，经 Rust 代理访问记忆服务） ----------
// 不再使用 iframe：WebView2 加载 http://127.0.0.1 iframe 不可靠（白屏），
// 改为 memory_api 命令（Rust 本地回环 + 令牌鉴权）直接读写记忆数据。

let memoryToken = "";

// 记忆 API 代理调用（path 必须以 /memory/ 开头）
async function memApi(path, method, body) {
  const r = await invoke("memory_api", { path, method: method || "GET", body: body ? JSON.stringify(body) : null });
  return JSON.parse(r || "{}");
}

const MEM_TYPE_LABEL = { fact: "事实", preference: "偏好", task: "任务", code_snippet: "代码", plan: "计划", constraint: "约束", user_defined: "自定义", phase: "阶段" };
const MEM_STATUS_LABEL = { active: "启用", paused: "暂停", probation: "观察", deprecated: "已归档" };

async function setupMemoryCard() {
  const list = $("#memory-list");
  const offline = $("#mem-offline");
  const stateEl = $("#mem-state");
  if (!list) return;
  if (typeof resetMemoryList === "function") resetMemoryList();
  try {
    const st = await invoke("memory_status").catch((e) => {
      if (stateEl) stateEl.textContent = "⚠ 读取记忆服务状态失败：" + String(e).slice(0, 60);
      return null;
    });
    if (!st || !st.running) {
      list.innerHTML = '<div class="fs-empty">记忆服务未运行</div>';
      if (offline) offline.classList.remove("hidden");
      if (stateEl) stateEl.textContent = "引擎启动后自动加载记忆服务";
      return;
    }
    memoryToken = st.token || "";
    if (offline) offline.classList.add("hidden");
    await refreshMemoryList();
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">读取失败：' + escapeHtml(String(e).slice(0, 80)) + "</div>";
    if (stateEl) stateEl.textContent = "⚠ " + String(e).slice(0, 60);
  }
}

async function refreshMemoryList() {
  const list = $("#memory-list");
  const stateEl = $("#mem-state");
  if (!list) return;
  try {
    const [blk, st] = await Promise.all([
      memApi("/memory/blocks", "GET"),
      memApi("/memory/status", "GET"),
    ]);
    const blocks = (blk && blk.blocks) || [];
    state.memoryCount = blocks.length;
    renderMemoryList(blocks);
    updateMemoryFrameHeight();
    // 更新主窗口水位条
    if (st) {
      const t = st.conversation_tokens;
      if (t != null) {
        updateMemBar(t);
      }
    }
    const stateEl2 = $("#mem-state");
    if (stateEl2) stateEl2.textContent = blocks.length ? "✅ 记忆服务已连接 · " + blocks.length + " 个记忆块" : "✅ 记忆服务已连接";
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">读取失败：' + escapeHtml(String(e).slice(0, 80)) + "</div>";
    if (stateEl) stateEl.textContent = "⚠ " + String(e).slice(0, 60);
  }
}

function renderMemoryList(blocks) {
  const list = $("#memory-list");
  if (!list) return;
  if (!blocks.length) {
    list.innerHTML = '<div class="fs-empty">还没有记忆块。<br>完成任务后可点下方「阶段总结」归档本阶段关键信息。</div>';
    return;
  }
  list.innerHTML = "";
  for (const b of blocks) {
    const el = document.createElement("div");
    el.className = "mem-card" + (b.status === "paused" ? " mem-paused" : "");
    const typeLabel = MEM_TYPE_LABEL[b.type] || b.type;
    const stars = "★".repeat(Math.min(5, b.importance || 0)) + '<span class="mem-star-empty">' + "★".repeat(Math.max(0, 5 - (b.importance || 0))) + "</span>";
    el.innerHTML =
      '<div class="mem-card-head">' +
        '<span class="mem-tag">' + escapeHtml(typeLabel) + "</span>" +
        '<span class="mem-imp">' + stars + "</span>" +
        (b.is_pinned ? '<span class="mem-pin" title="已置顶">🔒</span>' : "") +
        '<span class="mem-status">' + escapeHtml(MEM_STATUS_LABEL[b.status] || b.status) + "</span>" +
      "</div>" +
      '<div class="mem-card-content">' + escapeHtml(b.content || "") + "</div>";
    el.title = "创建于记忆池第 " + (b.source_round || "?") + " 轮 · " + (b.token_count || 0) + " tokens";
    list.appendChild(el);
  }
}

// 记忆面板高度自适应（块多时内部滚动）
function updateMemoryFrameHeight() {
  const wrap = $("#memory-frame-wrap");
  if (!wrap) return;
  const fp = document.querySelector(".file-panel");
  const fileH = fp ? fp.offsetHeight : 160;
  const n = state.memoryCount;
  let h;
  if (n == null || n === 0) {
    h = fileH;
  } else {
    h = Math.min(n, 3) * 64 + 60;
  }
  h = Math.max(140, Math.min(h, 400));
  wrap.style.height = h + "px";
}

function resetMemoryList() {
  // 占位
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

// ---------- 阶段总结（主应用内弹窗） ----------

async function openPhaseModal() {
  $("#phase-goal-main").value = "";
  $("#phase-preview-main").classList.add("hidden");
  $("#phase-preview-main").textContent = "";
  $("#btn-phase-confirm").disabled = true;
  $("#phase-new-thread-main").checked = false;
  $("#phase-gen-state").textContent = "";
  $("#modal-phase").classList.remove("hidden");
}

async function generatePhaseSummary() {
  const goal = $("#phase-goal-main").value.trim();
  const btn = $("#btn-phase-gen");
  const stateEl = $("#phase-gen-state");
  btn.disabled = true;
  btn.textContent = "生成中…";
  stateEl.textContent = "";
  try {
    const r = await memApi("/memory/phase/preview", "POST", { goal });
    if (!r.ok && !r.summary) throw new Error(r.message || "生成失败");
    state.phaseSummary = r.summary || "";
    const pv = $("#phase-preview-main");
    pv.classList.remove("hidden");
    pv.textContent = state.phaseSummary +
      (r.tokens != null ? "\n\n（约 " + r.tokens + " tokens · 基于 " + (r.blocks_used || 0) + " 个记忆块）" : "");
    $("#btn-phase-confirm").disabled = false;
    stateEl.textContent = "已生成，确认后旧记忆块将归档";
  } catch (e) {
    stateEl.textContent = "生成失败：" + String(e).slice(0, 60);
  } finally {
    btn.disabled = false;
    btn.textContent = "✨ 生成总结";
  }
}

async function confirmPhaseSummary() {
  const goal = $("#phase-goal-main").value.trim();
  const openNew = $("#phase-new-thread-main").checked;
  if (!state.phaseSummary) { toast("请先生成总结", "warn"); return; }
  try {
    const r = await memApi("/memory/phase/confirm", "POST", { goal, summary: state.phaseSummary, open_new_thread: openNew });
    $("#modal-phase").classList.add("hidden");
    toast(r.message || "阶段已归档", "ok");
    if (openNew && state.phaseSummary) {
      await startPhaseThread(state.phaseSummary);
    }
    await refreshMemoryList();
  } catch (e) {
    toast("归档失败：" + e, "err");
  }
}

// 阶段确认联动：开启新阶段对话（首条消息 = 阶段总结，带交接标记）
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
    addMsg("assistant",
      '<div class="tag-block tag-plan"><div class="tag-title">📋 阶段交接 · 新阶段开始</div>' +
      '<div class="cmd-block">' + escapeHtml(text) + "</div></div>");
    await invoke("send_message", { text: "【阶段交接】以下为上一阶段的总结，作为新阶段的起始上下文：\n" + text });
    toast("新阶段对话已开启（首条消息 = 阶段交接总结）", "ok");
    await loadSessions();
  } catch (e) {
    toast("开启新阶段对话失败：" + e, "err");
  }
}

// 删除工作区（当前工作区删除后自动切换到下一个）