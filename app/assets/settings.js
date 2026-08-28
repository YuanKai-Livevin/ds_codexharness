async function openSettings() {
  try {
    const s = await invoke("get_settings");
    state.settings = s;
    $("#set-provider").value = s.provider_name === "deepseek" ? "deepseek" : "custom";
    $("#set-base").value = s.base_url || "";
    $("#set-model").value = s.model || "";
    $("#set-winsandbox").value = s.windows_sandbox || "unelevated";
    $("#set-logdir").value = s.log_dir || "C:\\HARNESS\\logs";
    $("#set-noauth").checked = !!s.no_auth;
    $("#set-bridge").checked = !!s.use_bridge;
    $("#set-showcmd").checked = !!s.show_commands;
    $("#set-key").value = "";
    state.apiKey = "";
    updateWinSandboxTip();
    try {
      const hasKey = await invoke("has_api_key");
      $("#key-status").textContent = hasKey ? "（已保存，可自动启动引擎）" : "（未保存）";
    } catch (e) { /* ignore */ }
    // 显示当前实际使用的密钥（脱敏，前 6 + 后 4）
    try {
      const ki = await invoke("get_api_key_masked");
      $("#key-current").textContent = ki.present
        ? "当前使用：" + ki.masked + (ki.provider ? "（" + ki.provider + "）" : "")
        : "当前使用：未配置（请输入 Key 后保存）";
    } catch (e) { /* ignore */ }
    $("#modal-settings").classList.remove("hidden");
  } catch (e) {
    toast("读取设置失败：" + e, "err");
  }
}

function updateWinSandboxTip() {
  const v = $("#set-winsandbox").value;
  const tip = $("#winsandbox-tip");
  if (v === "unelevated") tip.textContent = "推荐：受限令牌沙箱，无需任何授权即可生效，足以把 Agent 限制在工作区内。";
  else tip.textContent = "增强沙箱：需要一次 UAC 授权，适合需要更精细读写控制的场景。";
}

// 测试模型连通性
async function testConnection() {
  const btn = $("#btn-test-conn");
  const res = $("#test-result");
  if (btn) btn.disabled = true;
  if (res) { res.className = "test-result"; res.textContent = "测试中…"; }
  try {
    // 优先用输入框里的 Key，否则用已保存的
    const key = state.apiKey || "";
    const r = await invoke("test_connection", { apiKey: key });
    if (res) {
      res.className = "test-result " + (r.ok ? "ok" : "fail");
      // 能力档案（R4）：显示探测到的模型能力
      const caps = [];
      if (typeof r.supports_responses === "boolean") caps.push("Responses API: " + (r.supports_responses ? "✔" : "✘"));
      if (typeof r.supports_chat === "boolean") caps.push("Chat Completions: " + (r.supports_chat ? "✔" : "✘"));
      if (typeof r.supports_reasoning === "boolean") caps.push("推理: " + (r.supports_reasoning ? "✔" : "✘"));
      if (typeof r.returns_usage === "boolean") caps.push("用量上报: " + (r.returns_usage ? "✔" : "✘"));
      if (r.suggestion === "use_bridge") caps.push("建议: 开启内置翻译层");
      if (r.suggestion === "direct") caps.push("建议: 直连（无需翻译层）");
      const capLine = caps.length ? "　" + caps.join(" · ") : "";
      res.textContent = (r.message || "") + capLine;
      // 若仅支持 chat 且未开翻译层 → 提示
      if (r.suggestion === "use_bridge" && !$("#set-bridge").checked) {
        toast("检测到网关仅支持 chat/completions，建议勾选「使用内置翻译层」", "warn");
      }
    }
  } catch (e) {
    if (res) { res.className = "test-result fail"; res.textContent = "测试失败：" + e; }
  }
  if (btn) btn.disabled = false;
}

async function saveSettings() {
  const s = {
    workspace_path: (state.settings && state.settings.workspace_path) || "",
    provider_name: $("#set-provider").value === "deepseek" ? "deepseek" : "custom",
    base_url: $("#set-base").value.trim(),
    model: $("#set-model").value.trim(),
    api_key_env: "OH_API_KEY",
    sandbox_mode: (state.settings && state.settings.sandbox_mode) || "workspace-write",
    windows_sandbox: $("#set-winsandbox").value,
    log_dir: $("#set-logdir").value.trim() || "C:\\HARNESS\\logs",
    no_auth: $("#set-noauth").checked,
    use_bridge: $("#set-bridge").checked,
    show_commands: $("#set-showcmd").checked,
    onboarded: true,
  };
  try {
    await invoke("save_settings", { settings: s });
    // 仅当用户本次填写了 Key 才更新；留空则保留已保存的 Key（防止误清除）
    if (state.apiKey) {
      await invoke("save_api_key", { apiKey: state.apiKey });
    }
    state.settings = s;
    $("#modal-settings").classList.add("hidden");
    const noAuth = s.no_auth || false;
    const hasKey = state.apiKey || (await invoke("has_api_key").catch(() => false));
    state.running = false;
    setEngineChip("off", "");
    if (hasKey || noAuth) {
      toast("设置已保存，正在启动引擎…");
      startEngine();
    } else {
      toast("设置已保存（未填写 API Key 且未勾选内网免密钥，引擎未启动）");
    }
    await refreshStatus();
    refreshFS();
    renderWorkspacePanel();
    // 模式切换器同步当前沙箱
    renderMode(modeFromSandbox(s.sandbox_mode));
  } catch (e) {
    toast("保存失败：" + e, "err");
  }
}

// ---------- 文件引用（@） ----------