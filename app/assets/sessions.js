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
