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