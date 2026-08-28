async function init() {
  // 绑定事件
  $("#btn-settings").addEventListener("click", openSettings);
  // R6 审计
  $("#btn-audit").addEventListener("click", openAudit);
  $("#btn-audit-close").addEventListener("click", () => $("#modal-audit").classList.add("hidden"));
  $("#btn-audit-export").addEventListener("click", exportAuditPackage);
  document.querySelectorAll(".audit-filter").forEach((b) => {
    b.addEventListener("click", () => setAuditFilter(b.dataset.cat));
  });
  $("#btn-settings-cancel").addEventListener("click", () => $("#modal-settings").classList.add("hidden"));
  $("#btn-settings-save").addEventListener("click", saveSettings);
  $("#btn-onboard-go").addEventListener("click", () => {
    $("#modal-onboard").classList.add("hidden");
    openSettings(); // 引导完成后直接进入设置，填写 API Key
  });
  $("#btn-ap-allow").addEventListener("click", () => resolveApproval("accept"));
  $("#btn-ap-deny").addEventListener("click", () => resolveApproval("decline"));
  $("#btn-test-conn").addEventListener("click", testConnection);
  $("#btn-send").addEventListener("click", send);
  $("#btn-interrupt").addEventListener("click", async () => {
    try { await invoke("interrupt"); } catch (e) { /* ignore */ }
  });
  // 权限模式切换
  document.querySelectorAll(".mode-option").forEach((opt) => {
    opt.addEventListener("click", () => switchMode(opt.dataset.mode));
  });
  // 初始化模式（读当前沙箱设置）
  const initS = await invoke("get_settings").catch(() => null);
  if (initS) {
    state.settings = initS;
    renderMode(modeFromSandbox(initS.sandbox_mode || "workspace-write"));
  }
  $("#btn-add-ws").addEventListener("click", async () => {
    const picked = await pickFolderWithHint();
    if (picked) switchWorkspace(picked);
  });
  $("#btn-new-session").addEventListener("click", newSession);
  $("#btn-fs-refresh").addEventListener("click", refreshFS);

  // SKILLS：标题点击折叠/展开
  $("#skills-head").addEventListener("click", () => {
    const body = $("#skills-body");
    const toggle = $("#skills-toggle");
    if (body.classList.contains("hidden")) {
      body.classList.remove("hidden");
      toggle.textContent = "▾ SKILLS 仓库";
      renderSkills();
    } else {
      body.classList.add("hidden");
      toggle.textContent = "▸ SKILLS 仓库";
    }
  });
  // SKILLS：＋ 添加菜单（创建自定义 / 导入）
  $("#btn-skills-add").addEventListener("click", (e) => {
    e.stopPropagation();
    const menu = $("#skills-add-menu");
    menu.classList.toggle("hidden");
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#skills-add-menu") && !e.target.closest("#btn-skills-add")) {
      $("#skills-add-menu").classList.add("hidden");
    }
  });
  document.querySelectorAll(".skills-add-item").forEach((item) => {
    item.addEventListener("click", () => {
      $("#skills-add-menu").classList.add("hidden");
      if (item.dataset.action === "create") {
        // 引导用自然语言描述技能
        const ta = $("#composer");
        ta.focus();
        ta.placeholder = "请用自然语言描述要创建的技能，例如：创建一个技能，把 Excel 按月份汇总并生成图表。描述后直接发送…";
        toast("请用自然语言描述要创建的技能，然后发送", "warn");
      } else {
        $("#modal-import-skill").classList.remove("hidden");
      }
    });
  });
  $("#btn-import-cancel").addEventListener("click", () => $("#modal-import-skill").classList.add("hidden"));
  $("#btn-import-pick").addEventListener("click", async () => {
    const picked = await pickFolderWithHint();
    if (!picked) return;
    toast("正在导入技能…");
    try {
      const imported = await invoke("import_skills", { dir: picked });
      if (imported && imported.length) {
        toast("已导入 " + imported.length + " 个技能：" + imported.join("、"));
      } else {
        toast("所选目录中未找到含 SKILL.md 的技能文件夹", "warn");
      }
    } catch (err) {
      toast("导入失败：" + err, "err");
    }
    $("#modal-import-skill").classList.add("hidden");
    renderSkills();
  });

  $("#set-winsandbox").addEventListener("change", updateWinSandboxTip);
  $("#set-key").addEventListener("input", (e) => { state.apiKey = e.target.value.trim(); });

  // 引用清除
  $("#refs-clear").addEventListener("click", () => { state.refs = []; renderRefs(); });

  // 内嵌记忆面板：打开按钮 / 重试 / iframe 阶段确认与尺寸联动
  $("#btn-mem-open").addEventListener("click", openMemoryPanel);
  $("#btn-mem-retry").addEventListener("click", setupMemoryCard);
  window.addEventListener("message", (ev) => {
    // 仅接受来自当前记忆服务来源（动态端口）的消息
    if (memoryOrigin && ev.origin !== memoryOrigin) return;
    const d = ev.data || {};
    if (d.type === "memory:phase-confirmed") {
      toast("阶段已归档" + (d.openNewThread ? "，正在开启新阶段对话…" : ""), "ok");
      if (d.openNewThread) startPhaseThread(d.summary || d.goal || "");
    } else if (d.type === "memory:size") {
      // 面板汇报记忆块数量 → 自适应高度（空=文件窗口高，有块=最多 3 张卡）
      if (typeof d.count === "number") {
        state.memoryCount = d.count;
        updateMemoryFrameHeight();
      }
    }
  });

  // 输入 @ 触发文件引用面板
  $("#composer").addEventListener("input", onComposerInput);
  $("#composer").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
    if (e.key === "Escape") closeMention();
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#mention-panel") && !e.target.closest("#composer")) {
      closeMention();
    }
  });

  await initEvents();
  await refreshStatus();
  if (state.workspace) refreshFS();
  await renderWorkspacePanel();
  await renderSkills();
  if (state.running) loadSessions();
  setupMemoryCard();
  updateChatEmptyBg();
  updateMemoryFrameHeight();
  // 记忆块数量变化 → 面板高度自适应（兜底轮询，避免遗漏 iframe 消息）
  setInterval(refreshMemoryCount, 20000);

  // 轮询引擎状态：无条件以后端为准同步 UI（兜底自动启动竞态与事件丢失）
  setInterval(async () => {
    try {
      const s = await invoke("get_status");
      if (s.running !== state.running) {
        state.running = s.running;
        setEngineChip(s.running ? "running" : "off", "");
        if (s.running) {
          loadSessions();
          if (state.workspace) refreshFS();
        }
      }
    } catch (e) { /* ignore */ }
  }, 1500);

  // 若已配置且引擎未启动（例如自动启动未成功），在设置页给出提示即可，不打扰主界面
  setTimeout(async () => {
    if (!state.running && state.settings && state.settings.onboarded) {
      const s = await invoke("get_settings").catch(() => null);
      if (s && s.onboarded) {
        toast("引擎未在运行：打开右上角「设置」可手动启动，或检查 API Key", "warn");
      }
    }
  }, 5000);

  // 首次使用引导
  try {
    const s = await invoke("get_settings");
    if (!s.onboarded) $("#modal-onboard").classList.remove("hidden");
  } catch (e) { /* ignore */ }
}

window.addEventListener("DOMContentLoaded", () => {
  window.addEventListener("error", (e) => {
    showFatalError("JS 错误：" + (e.message || e.type));
  });
  init().catch((err) => {
    console.error("[init error]", err);
    showFatalError("初始化失败：" + (err && err.message ? err.message : err));
  });
});