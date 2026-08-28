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
// R2：动态端口 + 会话令牌（来自 memory_status），不再硬编码 8765
let memoryOrigin = "http://127.0.0.1:8765";   // 默认值；启动后由 memory_status 刷新
let memoryToken = "";

async function setupMemoryCard() {
  const frame = $("#memory-frame");
  const offline = $("#mem-offline");
  if (!frame) return;
  try {
    const st = await invoke("memory_status").catch(() => null);
    if (st && st.running && st.port) {
      memoryOrigin = "http://127.0.0.1:" + st.port;
      memoryToken = st.token || "";
      const src = memoryOrigin + "/?token=" + encodeURIComponent(memoryToken);
      if (frame.src !== src) {
        frame.src = src;
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
  if (!memoryOrigin) return;
  try {
    const headers = {};
    if (memoryToken) headers["Authorization"] = "Bearer " + memoryToken;
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), 3000);
    const r = await fetch(memoryOrigin + "/api/memory/blocks", { headers, signal: ctl.signal }).catch(() => null);
    clearTimeout(timer);
    if (!r || !r.ok) return;
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