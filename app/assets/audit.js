// R6 审计日志界面：任务/工具/文件/审批/错误/引擎 事件流 + 诊断包导出（用户主动）。

const AUDIT_CAT_META = {
  task: { icon: "🎯", label: "任务" },
  tool: { icon: "💻", label: "工具" },
  file: { icon: "📝", label: "文件" },
  approval: { icon: "🔐", label: "审批" },
  error: { icon: "⚠️", label: "错误" },
  engine: { icon: "⚙️", label: "引擎" },
};

function auditState() {
  if (!state.audit) state.audit = { rows: [], filter: "all" };
  return state.audit;
}

function fmtTime(ts) {
  if (!ts) return "--";
  const d = new Date(ts);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const hm = String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0") + ":" + String(d.getSeconds()).padStart(2, "0");
  return sameDay ? hm : d.toLocaleDateString("zh-CN") + " " + hm;
}

function esc(s) { return escapeHtml(String(s == null ? "" : s)); }

// 详情 JSON → 展示行
function detailLines(row) {
  let d = {};
  try { d = JSON.parse(row.detail || "{}"); } catch (e) { /* ignore */ }
  const lines = [];
  const push = (k, v) => { if (v != null && v !== "" && v !== false) lines.push([k, v]); };
  switch (row.event) {
    case "task_start":
      push("目标", d.goal);
      push("模型", d.model);
      push("网关", d.gateway);
      push("工作区", d.workspace);
      break;
    case "task_end":
      push("结果", d.status === "completed" ? "完成 ✅" : (d.status === "interrupted" ? "中断 ⏹" : (d.status === "failed" ? "失败 ⚠️" : d.status)));
      if (row.tokens_in != null || row.tokens_out != null)
        push("Token", "输入 " + fmtTokens(row.tokens_in) + " / 输出 " + fmtTokens(row.tokens_out));
      if (row.duration_ms != null) push("耗时", (row.duration_ms / 1000).toFixed(1) + " 秒");
      if (row.cost != null) push("成本", "≈ $" + row.cost.toFixed(4));
      if (Array.isArray(d.files) && d.files.length) push("产出文件", d.files.length + " 个：" + d.files.slice(0, 5).map((f) => f.split("\n")[0]).join("、"));
      break;
    case "command_completed":
      push("命令", d.command);
      push("状态", d.status === "completed" ? "成功" : d.status);
      if (d.output_chars != null) push("输出", d.output_chars + " 字符");
      break;
    case "file_changed":
      push("变更", d.summary);
      break;
    case "approval_request":
      push("类型", d.kind);
      push("操作", d.command || d.reason || d.changes);
      break;
    case "approval_decision":
      push("决定", d.decision === "accept" ? "允许 ✅" : (d.decision === "decline" ? "拒绝 ⛔" : d.decision));
      push("请求号", d.request_id);
      break;
    case "approval_closed":
      push("请求号", d.request_id);
      push("说明", "审批请求已关闭");
      break;
    case "engine_start":
      push("模型", d.model);
      push("接口", d.base_url);
      if (d.use_bridge) push("翻译层", "已启用（本地网关 :" + (d.gateway_port || "?") + "）");
      if (d.no_auth) push("鉴权", "免密钥");
      push("工作区", d.workspace);
      break;
    case "engine_crashed":
    case "engine_start_failed":
    case "send_failed":
      push("错误", d.msg);
      break;
    default:
      push("详情", row.detail);
  }
  return lines;
}

async function openAudit() {
  $("#modal-audit").classList.remove("hidden");
  await loadAudit();
}

async function loadAudit() {
  const list = $("#audit-list");
  if (!list) return;
  list.innerHTML = '<div class="fs-empty">加载中…</div>';
  try {
    const rows = await invoke("audit_list", { limit: 400 });
    auditState().rows = rows || [];
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">加载失败：' + esc(e) + "</div>";
    return;
  }
  renderAudit();
}

function renderAudit() {
  const list = $("#audit-list");
  if (!list) return;
  const st = auditState();
  const rows = st.rows;
  const countEl = $("#audit-count");
  if (countEl) countEl.textContent = "共 " + rows.length + " 条";

  // 按任务分组（保留倒序）
  const groups = []; // {taskId, rows: []}
  const byTask = new Map();
  for (const r of rows) {
    if (r.category === "task" && r.event === "task_start" && r.task_id) {
      byTask.set(r.task_id, { taskId: r.task_id, rows: [] });
    }
  }
  for (const r of rows) {
    if (r.task_id && byTask.has(r.task_id)) {
      byTask.get(r.task_id).rows.push(r);
    } else {
      groups.push({ taskId: null, rows: [r] });
    }
  }
  for (const [tid, g] of byTask) {
    groups.push(g);
    void tid;
  }

  const filtered = groups.filter((g) => {
    if (st.filter === "all") return true;
    return g.rows.some((r) => r.category === st.filter);
  });

  if (!filtered.length) {
    list.innerHTML = '<div class="fs-empty">暂无审计记录（完成一次任务后会自动记录）。</div>';
    return;
  }

  list.innerHTML = "";
  for (const g of filtered) {
    const group = document.createElement("div");
    group.className = "audit-group";
    if (g.taskId) {
      const head = document.createElement("div");
      head.className = "audit-task-head";
      head.innerHTML = "🎯 任务 <code>" + esc(g.taskId.slice(0, 18)) + "</code> · " + fmtTime(g.rows[g.rows.length - 1].ts);
      group.appendChild(head);
    }
    for (const r of g.rows) {
      if (st.filter !== "all" && r.category !== st.filter) continue;
      group.appendChild(rowEl(r));
    }
    list.appendChild(group);
  }
}

function rowEl(r) {
  const meta = AUDIT_CAT_META[r.category] || { icon: "•", label: r.category };
  const el = document.createElement("div");
  el.className = "audit-row cat-" + r.category;
  const lines = detailLines(r);
  let body = "";
  for (const [k, v] of lines) {
    body += '<div class="audit-line"><span class="audit-k">' + esc(k) + "</span><span class=\"audit-v\">" + esc(v) + "</span></div>";
  }
  el.innerHTML =
    '<div class="audit-row-head">' +
      '<span class="audit-icon">' + meta.icon + "</span>" +
      '<span class="audit-event">' + esc(eventLabel(r.event)) + "</span>" +
      '<span class="audit-time">' + fmtTime(r.ts) + "</span>" +
    "</div>" + body;
  // 任务结束行：最终用户是否接受
  if (r.category === "task" && r.event === "task_end" && r.accepted == null) {
    const btnRow = document.createElement("div");
    btnRow.className = "audit-accept";
    const ok = document.createElement("button");
    ok.className = "btn btn-small btn-primary";
    ok.textContent = "✅ 接受结果";
    ok.addEventListener("click", async () => {
      try {
        await invoke("audit_accept", { taskId: r.task_id, accepted: true });
        toast("已记录：用户接受该任务结果");
        await loadAudit();
      } catch (e) { toast("记录失败：" + e, "err"); }
    });
    const no = document.createElement("button");
    no.className = "btn btn-small btn-ghost";
    no.textContent = "✖ 不认可";
    no.addEventListener("click", async () => {
      try {
        await invoke("audit_accept", { taskId: r.task_id, accepted: false });
        toast("已记录：用户不认可该任务结果");
        await loadAudit();
      } catch (e) { toast("记录失败：" + e, "err"); }
    });
    btnRow.append(ok, no);
    el.appendChild(btnRow);
  } else if (r.accepted != null) {
    const tag = document.createElement("div");
    tag.className = "audit-accepted";
    tag.textContent = r.accepted ? "✅ 用户已接受" : "✖ 用户不认可";
    el.appendChild(tag);
  }
  return el;
}

function eventLabel(ev) {
  const map = {
    task_start: "任务开始", task_end: "任务结束",
    command_completed: "工具调用完成", file_changed: "文件变更",
    approval_request: "审批请求", approval_decision: "审批决策", approval_closed: "审批关闭",
    engine_start: "引擎启动", engine_stop: "引擎停止",
    engine_crashed: "引擎崩溃", engine_start_failed: "启动失败", send_failed: "发送失败",
    approval_failed: "审批失败",
  };
  return map[ev] || ev;
}

function setAuditFilter(cat) {
  auditState().filter = cat;
  document.querySelectorAll(".audit-filter").forEach((b) => {
    b.classList.toggle("active", b.dataset.cat === cat);
  });
  renderAudit();
}

// 导出诊断包（用户主动；不含 API Key，日志已脱敏）
async function exportAuditPackage() {
  try {
    toast("请选择保存位置…");
    const path = await invoke("audit_export");
    toast("诊断包已导出：" + path, "ok");
  } catch (e) {
    if (String(e).indexOf("取消") >= 0) return;
    toast("导出失败：" + e, "err");
  }
}
