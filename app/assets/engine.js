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
      // T0-05：启动中状态跟踪（供「■ 停止」按钮取消启动）
      state.starting = ev.state === "starting";
      if (ev.state === "starting") {
        showRunBar("正在启动引擎…（可点「■ 停止」取消）", null);
      }
      if (ev.state === "running") {
        state.running = true;
        hideRunBar();
        // 引擎就绪后加载记忆面板（自动启动路径下 boot 时服务可能还没起来，这里兜底重试）
        setupMemoryCard();
      }
      if (ev.state === "error") {
        state.running = false;
        hideRunBar();
        toast(ev.detail || "引擎启动失败", "err");
        // 引导用户补全配置
        if (ev.detail && ev.detail.includes("API Key")) openSettings();
      }
      if (ev.state === "failed") {
        state.running = false;
        hideRunBar();
        toast(ev.detail || "引擎异常退出", "err");
      }
      if (ev.state === "stopped") {
        hideRunBar();
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
      state.currentReason = null;   // 每回合新的思考块
      resetPlan();
      showRunBar("任务运行中…", null);
      break;
    case "agentDelta":
      routeAgentText(ev.text);
      break;
    case "agentMessage":
      routeAgentText(ev.text);
      break;
    case "reasoningDelta":
      appendReason(ev.text);
      break;
    case "commandStarted": {
      // 关闭「显示命令细节」时：只驱动任务计划进度，不渲染命令块
      if (!showCommandsSetting()) {
        advancePlan("running", ev.command);
        break;
      }
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
      if (showCommandsSetting() && state.currentCmd && state.currentCmd.itemId === ev.itemId) {
        state.currentCmd.out.textContent += ev.output;
        state.currentCmd._hasOut = true;
        if (state.currentCmd._open) {
          scrollBottom();
        }
      }
      break;
    case "commandCompleted":
      if (!showCommandsSetting()) {
        advancePlan(ev.status === "completed" ? "done" : "failed", ev.command);
        break;
      }
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
      // 收尾任务计划：未闭合的计划段先成卡，再按结果标记全部步骤
      if (state.planPhase === "plan") finalizePlan();
      if (state.plan && state.plan.length) {
        const ok = ev.status === "completed";
        state.plan.forEach((s) => {
          if (s.status === "pending" || s.status === "running") {
            s.status = ok ? "done" : "failed";
            renderPlanStep(s);
          }
        });
      }
      finishReasonPanel(ev.status);
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
    // 单一状态机：ready/busy → 运行中；failed → 失败；starting → 启动中；其余 → 停止
    const es = s.engine_state || (s.running ? "ready" : "stopped");
    if (es === "ready" || es === "busy") {
      setEngineChip("running", "");
    } else if (es === "starting") {
      setEngineChip("starting", "");
    } else if (es === "failed") {
      setEngineChip("failed", "");
    } else {
      setEngineChip("off", "");
    }
    if (!s.python_ok || !s.codex_ok) {
      toast("缺少运行组件：请把本程序与 runtime/、codex-bin/ 放在同一目录", "warn");
    }
  } catch (e) {
    console.error("refreshStatus", e);
  }
}

// ---------- SKILLS 仓库 ----------
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
    state.starting = true;
    showRunBar("正在启动引擎…（可点「■ 停止」取消）", null);
    await invoke("start_engine", { apiKey: key || "" });
    state.running = true;
    state.starting = false;
    setEngineChip("running", "");
    hideRunBar();
    toast("引擎已启动");
    setupMemoryCard(); // 记忆服务随引擎启动，加载内嵌面板
    return true;
  } catch (e) {
    state.running = false;
    state.starting = false;
    hideRunBar();
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

// ---------- 任务计划与进度（DSH 风格） ----------

// 是否显示命令细节（设置开关，默认隐藏）
function showCommandsSetting() {
  return !!(state.settings && state.settings.show_commands);
}

function resetPlan() {
  state.plan = null;
  state.planEl = null;
  state.planRaw = "";
  state.planPhase = "text";
}

// 把助手文本流按「普通文本 / 【执行计划】」分流：
// 计划内容进计划卡，其余文字进模型气泡
function routeAgentText(text) {
  const TAG = /【([^】]+)】/;
  let rest = text || "";
  while (rest.length) {
    if (state.planPhase === "plan") {
      const m = TAG.exec(rest);
      if (m && m.index === 0) {
        // 下一标签开始 → 计划段结束，成卡；标签本身留在普通文本
        finalizePlan();
        state.planPhase = "text";
        continue;
      }
      if (m && m.index > 0) {
        state.planRaw += rest.slice(0, m.index);
        rest = rest.slice(m.index);
        continue;
      }
      state.planRaw += rest;
      rest = "";
    } else {
      const idx = rest.indexOf("【执行计划】");
      if (idx >= 0) {
        if (idx > 0) appendAssistantText(rest.slice(0, idx));
        state.planPhase = "plan";
        rest = rest.slice(idx + "【执行计划】".length);
        continue;
      }
      appendAssistantText(rest);
      rest = "";
    }
  }
  scrollBottom();
}

function appendAssistantText(t) {
  if (!t) return;
  if (!state.currentAssistant) {
    state.currentAssistant = addMsg("assistant", "");
  }
  renderAssistantText(state.currentAssistant, t);
}

// 计划文本 → 步骤数组（编号/圆点前缀剥离，每非空行一步，上限 12）
function parsePlanSteps(raw) {
  const steps = [];
  for (const line of (raw || "").split("\n")) {
    const t = line.trim();
    if (!t) continue;
    const m = t.match(/^(?:\d+[\.、\)]|[-*•])\s*(.*)$/);
    steps.push({ text: (m ? m[1] : t).slice(0, 80), status: "pending" });
    if (steps.length >= 12) break;
  }
  return steps;
}

// 渲染「📋 任务计划」卡片（插在模型气泡之后）
function finalizePlan() {
  const raw = state.planRaw || "";
  const steps = parsePlanSteps(raw);
  state.planRaw = "";
  if (!steps.length || state.planEl) return;
  state.plan = steps;
  const card = document.createElement("div");
  card.className = "plan-card collapsed";
  // 头部：默认折叠，点击展开/收起；实时显示进度摘要
  const title = document.createElement("div");
  title.className = "plan-title";
  title.innerHTML = '<span class="plan-toggle">▸</span> <span class="plan-title-text">📋 任务计划（' + steps.length + " 步）</span>" +
    '<span class="plan-summary"></span>';
  title.addEventListener("click", () => {
    card.classList.toggle("collapsed");
    title.querySelector(".plan-toggle").textContent = card.classList.contains("collapsed") ? "▸" : "▾";
  });
  card.appendChild(title);
  const stepsBox = document.createElement("div");
  stepsBox.className = "plan-steps";
  for (const s of steps) {
    const row = document.createElement("div");
    row.className = "plan-step plan-pending";
    const dot = document.createElement("span");
    dot.className = "plan-dot";
    dot.textContent = "○";
    const txt = document.createElement("span");
    txt.textContent = s.text;
    row.append(dot, txt);
    s.dom = row;
    s.dot = dot;
    stepsBox.appendChild(row);
  }
  card.appendChild(stepsBox);
  const anchor = state.currentAssistant ? state.currentAssistant.parentElement : null;
  if (anchor && anchor.parentElement) {
    anchor.insertAdjacentElement("afterend", card);
  } else {
    $("#messages").appendChild(card);
  }
  state.planEl = card;
  updatePlanSummary();
  scrollBottom();
}

// 折叠头部显示进度摘要：已完成 X / N 步 + 当前步骤
function updatePlanSummary() {
  if (!state.planEl) return;
  const sum = state.planEl.querySelector(".plan-summary");
  if (!sum) return;
  const done = state.plan.filter((s) => s.status === "done").length;
  const failed = state.plan.filter((s) => s.status === "failed").length;
  const run = state.plan.find((s) => s.status === "running");
  let text = "已完成 " + done + " / " + state.plan.length + " 步";
  if (failed) text += " · " + failed + " 步失败";
  if (run) text += " · ⟳ " + run.text.slice(0, 18) + (run.text.length > 18 ? "…" : "");
  sum.textContent = text;
}

function renderPlanStep(s) {
  if (!s || !s.dom) return;
  const map = {
    pending: ["plan-pending", "○"],
    running: ["plan-running", "⟳"],
    done: ["plan-done", "✔"],
    failed: ["plan-failed", "✘"],
  };
  const [cls, dot] = map[s.status] || map.pending;
  s.dom.className = "plan-step " + cls;
  if (s.dot) s.dot.textContent = dot;
  updatePlanSummary();
}

// 命令事件驱动计划进度（命令细节隐藏时）
function advancePlan(status, cmd) {
  void cmd;
  if (status === "running") {
    const step = state.plan && state.plan.find((s) => s.status === "pending");
    if (step) {
      step.status = "running";
      renderPlanStep(step);
      showRunBar("正在执行：" + step.text.slice(0, 28) + (step.text.length > 28 ? "…" : ""), null);
    } else {
      showRunBar("任务执行中…", null);
    }
    return;
  }
  // done / failed
  const step = state.plan
    ? (state.plan.find((s) => s.status === "running") || state.plan.find((s) => s.status === "pending"))
    : null;
  if (step) {
    step.status = status === "done" ? "done" : "failed";
    renderPlanStep(step);
    showRunBar((status === "done" ? "✔ " : "⚠ ") + step.text.slice(0, 28) + (step.text.length > 28 ? "…" : ""), null);
  }
}

// ---------- 主消息流思考块（顶部，首个推理增量时创建，持续追加） ----------

// 在当前回合的用户消息之后插入思考块（保证位于计划卡/结果之上）
function createReasonSlot() {
  const msgs = $("#messages");
  const users = msgs.querySelectorAll(":scope > .msg.user");
  const anchor = users.length ? users[users.length - 1] : null;
  const wrap = document.createElement("div");
  wrap.className = "reasoning";
  wrap.innerHTML = '<div class="r-head">🧠 思考过程（实时，点击折叠）</div><div class="r-body"></div>';
  wrap.querySelector(".r-head").addEventListener("click", () => {
    wrap.classList.toggle("closed");
  });
  if (anchor) {
    anchor.insertAdjacentElement("afterend", wrap);
  } else {
    msgs.appendChild(wrap);
  }
  return wrap;
}

function appendReason(text) {
  if (!state.currentReason) {
    state.currentReason = createReasonSlot();
  }
  const body = state.currentReason.querySelector(".r-body");
  body.textContent += text || "";
  body.scrollTop = body.scrollHeight;
  scrollBottom();
}

function finishReasonPanel(status) {
  if (!state.currentReason) return;
  const body = state.currentReason.querySelector(".r-body");
  const sep = document.createElement("div");
  sep.className = "r-end";
  sep.textContent = "—— 本轮思考结束" + (status === "completed" ? "" : "（" + (status || "未完成") + "）") + " ——";
  body.appendChild(sep);
  body.scrollTop = body.scrollHeight;
  state.currentReason = null;
}

// ---------- 初始化 ----------