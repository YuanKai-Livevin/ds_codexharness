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
    // 加载历史消息（含工具调用/命令输出/文件变更/轮次状态）
    try {
      const history = await invoke("session_history", { threadId: id });
      renderHistory(history);
    } catch (e2) { /* 历史加载失败不影响切换 */ }
    await loadSessions();
    toast("已切换到历史会话");
  } catch (e) {
    toast("切换会话失败：" + e, "err");
  }
}

// 恢复历史消息：文本 / 命令块（可展开输出）/ 文件变更 / 轮次状态徽标
function renderHistory(history) {
  let lastStatus = "";
  let lastEl = null;
  let cmdCount = 0;
  const showCmds = !!(state.settings && state.settings.show_commands);
  for (const h of history || []) {
    if (h.status && h.status !== lastStatus) {
      if (lastEl) appendTurnStatus(lastEl, lastStatus);
      lastStatus = h.status;
      lastEl = null;
    }
    if (h.kind === "command") {
      cmdCount++;
      if (!showCmds) {
        // 隐藏命令细节：只计数，不渲染命令块
        continue;
      }
      const wrap = buildHistoryCmdBlock(h.command, h.output, h.status);
      $("#messages").appendChild(wrap);
      lastEl = wrap;
    } else if (h.kind === "filechange") {
      const wrap = document.createElement("div");
      wrap.className = "msg assistant";
      const b = document.createElement("div");
      b.className = "bubble";
      const sum = escapeHtml((h.text || "").slice(0, 300)) + ((h.text || "").length > 300 ? "…" : "");
      b.innerHTML = '<div class="tag-block tag-plan"><div class="tag-title">📝 文件修改</div><div class="cmd-block">' + sum + "</div></div>";
      wrap.appendChild(b);
      $("#messages").appendChild(wrap);
      lastEl = wrap;
    } else if (h.role === "user") {
      lastEl = addMsg("user", renderMd(h.text));
    } else {
      const b = addMsg("assistant", "");
      renderAssistantText(b, h.text);
      lastEl = b.parentElement;
    }
  }
  if (lastEl && lastStatus) appendTurnStatus(lastEl, lastStatus);
  // 隐藏命令细节时：追加一行「共执行 N 次工具调用」统计
  if (!showCmds && cmdCount > 0) {
    const line = document.createElement("div");
    line.className = "turn-status";
    line.textContent = "💻 共执行 " + cmdCount + " 次工具调用（在设置中打开「显示命令执行细节」可查看）";
    $("#messages").appendChild(line);
  }
  scrollBottom();
  updateChatEmptyBg();
}

// 历史命令块：默认折叠，点击展开输出；按轮次状态标注 ✔/✘
function buildHistoryCmdBlock(command, output, status) {
  const wrap = document.createElement("div");
  wrap.className = "cmd-wrap";
  const cmd = command || "";
  const ok = status !== "failed" && status !== "interrupted";
  const head = document.createElement("div");
  head.className = "cmd-head";
  head.textContent = (ok ? "✔ " : "✘ ") + "💻 " + cmd.slice(0, 140) + (cmd.length > 140 ? "…" : "");
  head.title = cmd + "\n（点击展开/收起输出）";
  const out = document.createElement("div");
  out.className = "cmd-output";
  out.style.display = "none";
  out.textContent = (output || "").trim() || "（无输出）";
  head.addEventListener("click", () => {
    out.style.display = out.style.display === "none" ? "" : "none";
    scrollBottom();
  });
  wrap.append(head, out);
  return wrap;
}

// 轮次结束状态徽标
function appendTurnStatus(afterEl, status) {
  const tag = document.createElement("div");
  tag.className = "turn-status";
  const map = {
    completed: "✅ 本回合完成",
    failed: "⚠ 本回合失败",
    interrupted: "⏹ 本回合已中断",
    requiresAction: "🔶 等待确认",
  };
  tag.textContent = map[status] || "本回合：" + status;
  afterEl.insertAdjacentElement("afterend", tag);
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
