// R9 任务与产出物界面：任务卡（目标/状态/输入/产出/审批）+ 产出物卡（打开/定位/Diff）。

function tasksState() {
  if (!state.tasks) state.tasks = { rows: [], filter: "all", artifacts: {} };
  return state.tasks;
}

async function openTasks() {
  $("#modal-tasks").classList.remove("hidden");
  await loadTasks();
}

async function loadTasks() {
  const list = $("#tasks-list");
  if (!list) return;
  list.innerHTML = '<div class="fs-empty">加载中…</div>';
  let rows;
  try {
    rows = await invoke("audit_list", { limit: 500 });
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">加载失败：' + esc(e) + "</div>";
    return;
  }
  tasksState().rows = rows || [];
  renderTasks();
}

// 审计事件行 → 任务模型
function buildTasks(rows) {
  const tasks = new Map();
  const get = (tid) => {
    if (!tasks.has(tid)) {
      tasks.set(tid, {
        taskId: tid,
        goal: "", status: "running", ts: 0, endTs: 0,
        tokensIn: null, tokensOut: null, durationMs: null, cost: null,
        workspace: "", threadId: null, files: [], tools: [], approvals: 0, denied: 0, errors: [],
        accepted: null,
      });
    }
    return tasks.get(tid);
  };
  for (const r of rows) {
    if (!r.task_id) continue;
    const t = get(r.task_id);
    if (r.ts) t.ts = Math.max(t.ts, r.ts);
    if (r.category === "task") {
      let d = {};
      try { d = JSON.parse(r.detail || "{}"); } catch (e) { /* ignore */ }
      if (r.event === "task_start") {
        t.goal = d.goal || "";
        t.workspace = d.workspace || t.workspace;
        t.threadId = d.thread_id || t.threadId;
        t.ts = r.ts;
      } else if (r.event === "task_end") {
        t.status = d.status || "completed";
        t.endTs = r.ts;
        t.tokensIn = r.tokens_in; t.tokensOut = r.tokens_out;
        t.durationMs = r.duration_ms; t.cost = r.cost;
        t.accepted = r.accepted;
        if (Array.isArray(d.files)) t.files = t.files.concat(d.files);
      }
    } else if (r.category === "file" && r.event === "file_changed") {
      let d = {};
      try { d = JSON.parse(r.detail || "{}"); } catch (e) { /* ignore */ }
      if (d.summary) t.files.push(d.summary);
    } else if (r.category === "tool") {
      let d = {};
      try { d = JSON.parse(r.detail || "{}"); } catch (e) { /* ignore */ }
      t.tools.push({ command: d.command || "", status: d.status || "", output_chars: d.output_chars });
    } else if (r.category === "approval") {
      let d = {};
      try { d = JSON.parse(r.detail || "{}"); } catch (e) { /* ignore */ }
      if (r.event === "approval_request") t.approvals++;
      if (r.event === "approval_decision" && d.decision === "decline") t.denied++;
    } else if (r.category === "error") {
      let d = {};
      try { d = JSON.parse(r.detail || "{}"); } catch (e) { /* ignore */ }
      if (d.msg) t.errors.push(String(d.msg));
    }
  }
  return [...tasks.values()];
}

function statusChip(t) {
  if (t.status === "completed") return '<span class="chip chip-ok">✅ 已完成</span>';
  if (t.status === "failed") return '<span class="chip chip-warn">⚠️ 失败</span>';
  if (t.status === "interrupted") return '<span class="chip chip-warn">⏹ 已中断</span>';
  return '<span class="chip chip-run">⏳ 进行中</span>';
}

function renderTasks() {
  const list = $("#tasks-list");
  if (!list) return;
  const st = tasksState();
  let tasks = buildTasks(st.rows);
  tasks.sort((a, b) => b.ts - a.ts);
  if (st.filter === "running") tasks = tasks.filter((t) => t.status === "running");
  if (st.filter === "completed") tasks = tasks.filter((t) => t.status === "completed");
  if (st.filter === "failed") tasks = tasks.filter((t) => t.status === "failed" || t.status === "interrupted");
  $("#tasks-count").textContent = "共 " + tasks.length + " 个任务";

  if (!tasks.length) {
    list.innerHTML = '<div class="fs-empty">暂无任务记录。发送一条消息后，这里会显示任务卡片与产出物。</div>';
    return;
  }
  list.innerHTML = "";
  for (const t of tasks) list.appendChild(taskCard(t));
}

function taskCard(t) {
  const el = document.createElement("div");
  el.className = "task-card";
  const goal = t.goal || "（无目标记录）";
  const goalShort = goal.length > 90 ? goal.slice(0, 90) + "…" : goal;
  const meta = [];
  if (t.tokensIn != null || t.tokensOut != null)
    meta.push("Token " + fmtTokens(t.tokensIn) + "/" + fmtTokens(t.tokensOut));
  if (t.durationMs != null) meta.push((t.durationMs / 1000).toFixed(1) + "s");
  if (t.cost != null) meta.push("$" + t.cost.toFixed(4));
  if (t.tools.length) meta.push(t.tools.length + " 次工具调用");
  if (t.approvals) meta.push(t.approvals + " 次审批" + (t.denied ? "（拒绝 " + t.denied + "）" : ""));
  const errBadge = t.errors.length ? '<span class="task-err">' + t.errors.length + " 个错误</span>" : "";

  el.innerHTML =
    '<div class="task-card-head">' +
      statusChip(t) + " " + errBadge +
      '<span class="task-time">' + fmtTime(t.ts) + "</span>" +
    "</div>" +
    '<div class="task-goal" title="' + esc(goal) + '">' + esc(goalShort) + "</div>" +
    (meta.length ? '<div class="task-meta">' + meta.join(" · ") + "</div>" : "");

  // 按钮行
  const btns = document.createElement("div");
  btns.className = "task-btns";
  const mkBtn = (label, cls, fn) => {
    const b = document.createElement("button");
    b.className = "btn btn-small " + cls;
    b.textContent = label;
    b.addEventListener("click", fn);
    btns.appendChild(b);
  };
  if (t.threadId && t.status !== "running") {
    mkBtn("📄 产出物", "btn-primary", () => toggleArtifacts(el, t));
  }
  if (t.accepted == null && t.status !== "running") {
    mkBtn("✅ 接受结果", "", () => acceptTask(t, true));
    mkBtn("✖ 不认可", "btn-ghost", () => acceptTask(t, false));
  }
  mkBtn("📋 复制目标", "btn-ghost", () => {
    const c = $("#composer");
    if (c) { c.value = t.goal; c.focus(); toast("已复制任务目标到输入框"); }
  });
  el.appendChild(btns);

  // 产出物容器（懒加载）
  const arts = document.createElement("div");
  arts.className = "task-artifacts hidden";
  el.appendChild(arts);
  return el;
}

async function toggleArtifacts(cardEl, t) {
  const arts = cardEl.querySelector(".task-artifacts");
  const isHidden = arts.classList.contains("hidden");
  arts.classList.toggle("hidden");
  if (!isHidden) return;
  arts.innerHTML = '<div class="fs-empty">读取产出物…</div>';
  let changes;
  try {
    changes = await invoke("task_artifacts", { threadId: t.threadId, turnId: t.taskId });
  } catch (e) {
    arts.innerHTML = '<div class="fs-empty">读取失败：' + esc(e) + "</div>";
    return;
  }
  if (!changes || !changes.length) {
    arts.innerHTML = '<div class="fs-empty">该任务没有文件变更记录。</div>';
    return;
  }
  arts.innerHTML = "";
  for (const ch of changes) arts.appendChild(artifactCard(ch, t));
}

function extIcon(path) {
  const ext = (path.split(".").pop() || "").toLowerCase();
  const m = {
    xlsx: "📊", xls: "📊", csv: "📊", docx: "📝", doc: "📝", pptx: "📑", ppt: "📑",
    pdf: "📕", md: "📄", txt: "📄", json: "🧾", py: "🐍", png: "🖼", jpg: "🖼", jpeg: "🖼",
  };
  return m[ext] || "📎";
}

function artifactCard(ch, t) {
  const el = document.createElement("div");
  el.className = "artifact-card";
  const name = ch.path.split(/[\\/]/).pop() || ch.path;
  const ctLabel = ch.change_type === "created" ? "新建" : ch.change_type === "modified" ? "修改" : ch.change_type === "deleted" ? "删除" : (ch.change_type || "变更");
  el.innerHTML =
    '<div class="artifact-head">' +
      '<span class="artifact-icon">' + extIcon(name) + "</span>" +
      '<span class="artifact-name" title="' + esc(ch.path) + '">' + esc(name) + "</span>" +
      '<span class="chip chip-warn">' + esc(ctLabel) + "</span>" +
    "</div>";
  const btns = document.createElement("div");
  btns.className = "task-btns";
  const fullPath = (t.workspace ? t.workspace.replace(/[\\/]$/, "") + "\\" : "") + ch.path.replace(/\//g, "\\");
  const bOpen = document.createElement("button");
  bOpen.className = "btn btn-small";
  bOpen.textContent = "打开";
  bOpen.addEventListener("click", () => invoke("open_path", { path: fullPath, reveal: false })
    .catch((e) => toast(String(e), "err")));
  const bReveal = document.createElement("button");
  bReveal.className = "btn btn-small btn-ghost";
  bReveal.textContent = "定位";
  bReveal.addEventListener("click", () => invoke("open_path", { path: fullPath, reveal: true })
    .catch((e) => toast(String(e), "err")));
  btns.append(bOpen, bReveal);
  if (ch.old || ch.new) {
    const bDiff = document.createElement("button");
    bDiff.className = "btn btn-small btn-ghost";
    bDiff.textContent = "Diff";
    bDiff.addEventListener("click", () => toggleDiff(el, ch));
    btns.appendChild(bDiff);
  }
  el.appendChild(btns);
  const diffBox = document.createElement("div");
  diffBox.className = "artifact-diff hidden";
  el.appendChild(diffBox);
  return el;
}

function toggleDiff(cardEl, ch) {
  const box = cardEl.querySelector(".artifact-diff");
  box.classList.toggle("hidden");
  if (box.classList.contains("hidden")) return;
  if (!box.dataset.rendered) {
    box.innerHTML = renderDiff(ch.old || "", ch.new || "");
    box.dataset.rendered = "1";
  }
}

// 行级 diff（LCS DP，上限 600 行；超出则并排展示）
function renderDiff(oldText, newText) {
  const oldLines = oldText.replace(/\r\n/g, "\n").split("\n");
  const newLines = newText.replace(/\r\n/g, "\n").split("\n");
  if (oldLines.length > 600 || newLines.length > 600) {
    return '<div class="diff-grid"><div class="diff-col"><div class="diff-col-title">修改前</div><pre class="diff-pre">' +
      esc(oldText) + "</pre></div><div class=\"diff-col\"><div class=\"diff-col-title\">修改后</div><pre class=\"diff-pre\">" +
      esc(newText) + "</pre></div></div>";
  }
  // LCS
  const n = oldLines.length, m = newLines.length;
  const dp = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i][j] = oldLines[i] === newLines[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  let html = '<div class="diff-view">';
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (oldLines[i] === newLines[j]) {
      html += '<div class="diff-line diff-ctx"><span class="diff-num">' + (i + 1) + "</span>" + esc(oldLines[i]) + "</div>";
      i++; j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      html += '<div class="diff-line diff-del"><span class="diff-num">' + (i + 1) + "</span>- " + esc(oldLines[i]) + "</div>";
      i++;
    } else {
      html += '<div class="diff-line diff-add"><span class="diff-num">' + (j + 1) + "</span>+ " + esc(newLines[j]) + "</div>";
      j++;
    }
  }
  while (i < n) { html += '<div class="diff-line diff-del"><span class="diff-num">' + (i + 1) + "</span>- " + esc(oldLines[i]) + "</div>"; i++; }
  while (j < m) { html += '<div class="diff-line diff-add"><span class="diff-num">' + (j + 1) + "</span>+ " + esc(newLines[j]) + "</div>"; j++; }
  html += "</div>";
  return html;
}

async function acceptTask(t, accepted) {
  try {
    await invoke("audit_accept", { taskId: t.taskId, accepted });
    toast(accepted ? "已记录：接受该任务结果" : "已记录：不认可该任务结果");
    await loadTasks();
  } catch (e) { toast("记录失败：" + e, "err"); }
}

function setTaskFilter(st) {
  tasksState().filter = st;
  document.querySelectorAll(".task-filter").forEach((b) => b.classList.toggle("active", b.dataset.st === st));
  renderTasks();
}
