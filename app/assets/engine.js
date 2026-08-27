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
      if (ev.state === "running") {
        state.running = true;
      }
      if (ev.state === "error") {
        state.running = false;
        toast(ev.detail || "引擎启动失败", "err");
        // 引导用户补全配置
        if (ev.detail && ev.detail.includes("API Key")) openSettings();
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
      showRunBar("任务运行中…", null);
      break;
    case "agentDelta":
      if (!state.currentAssistant) {
        state.currentAssistant = addMsg("assistant", "");
      }
      renderAssistantText(state.currentAssistant, ev.text);
      scrollBottom();
      break;
    case "agentMessage":
      if (!state.currentAssistant) {
        state.currentAssistant = addMsg("assistant", "");
      }
      renderAssistantText(state.currentAssistant, ev.text);
      scrollBottom();
      break;
    case "reasoningDelta":
      if (!state.currentReason) {
        state.currentReason = addMsg("assistant", "", "reasoning");
        state.currentReason.innerHTML =
          '<div class="r-head">🧠 思考摘要（点击展开）</div><div class="r-body"></div>';
        state.currentReason.querySelector(".r-head").addEventListener("click", () => {
          state.currentReason.classList.toggle("open");
        });
      }
      state.currentReason.querySelector(".r-body").textContent += ev.text;
      break;
    case "commandStarted": {
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
      if (state.currentCmd && state.currentCmd.itemId === ev.itemId) {
        state.currentCmd.out.textContent += ev.output;
        state.currentCmd._hasOut = true;
        if (state.currentCmd._open) {
          scrollBottom();
        }
      }
      break;
    case "commandCompleted":
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
      if (state.currentReason) {
        state.currentReason.querySelector(".r-head").textContent = "🧠 思考摘要（点击展开）";
        state.currentReason = null;
      }
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
    setEngineChip(s.running ? "running" : "off", "");
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
    await invoke("start_engine", { apiKey: key || "" });
    state.running = true;
    setEngineChip("running", "");
    toast("引擎已启动");
    setupMemoryCard(); // 记忆服务随引擎启动，加载内嵌面板
    return true;
  } catch (e) {
    state.running = false;
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

// ---------- 初始化 ----------