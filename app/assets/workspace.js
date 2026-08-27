function fsCrumbText(path) {
  const ws = state.workspace.replace(/\\+$/, "");
  if (!path || path === ws) return "📁 " + (ws.split(/[\\/]/).pop() || ws);
  return "📁 " + path.slice(ws.length).replace(/^[\\/]/, "").replace(/[\\/]/g, " / ");
}

// ---------- 工作区切换（dsh 风格：SKILLS 仓库固定 + 我的工作区） ----------
async function renderWorkspacePanel() {
  const s = await invoke("get_settings").catch(() => null);
  if (!s) return;
  state.settings = s;
  // 若工作区路径是文件而非文件夹，给出醒目警告
  const warn = $("#ws-warn");
  if (s.workspace_path) {
    const r = await invoke("list_dir", { path: s.workspace_path }).catch((e) => String(e));
    if (typeof r === "string" && r.includes("不是目录")) {
      warn.textContent = "⚠ 当前工作区路径不是文件夹，请重新添加工作区。";
      warn.classList.remove("hidden");
    } else {
      warn.classList.add("hidden");
    }
  }
  const recentEl = $("#ws-recent");
  recentEl.innerHTML = "";

  // 固定项：SKILLS 仓库（永远存在、不可删除）
  let skillsPath = state.skillsRepo;
  if (!skillsPath) {
    try {
      const info = await invoke("get_skills_repo");
      skillsPath = info.path || "";
    } catch (e) { /* ignore */ }
  }
  if (skillsPath) {
    recentEl.appendChild(wsItem(skillsPath, "🧩", skillsPath === s.workspace_path, true));
  }

  // 固定项：记忆面板（记忆块管理 + 上下文压缩/交接 + 真实水位）
  const memItem = document.createElement("div");
  memItem.className = "ws-item mem-item";
  memItem.innerHTML =
    '<span class="ws-icon">🧠</span>' +
    '<span class="ws-name">记忆面板</span>' +
    '<span class="ws-sub">上下文水位 · 压缩 · 交接</span>';
  memItem.title = "打开记忆面板：记忆块管理、自动压缩与交接文档（当前工作区）";
  memItem.addEventListener("click", () => openMemoryPanel());
  recentEl.appendChild(memItem);

  // 我的工作区列表：当前工作区 + 已添加的其他工作区
  const all = [s.workspace_path].concat((s.recent_workspaces || []).filter((r) => r && r !== s.workspace_path));
  const uniq = [];
  for (const w of all) {
    if (w && w !== skillsPath && !uniq.includes(w)) uniq.push(w);
  }
  if (uniq.length) {
    const label = document.createElement("div");
    label.className = "ws-label";
    label.textContent = "我的工作区";
    recentEl.appendChild(label);
  }
  for (const w of uniq) {
    recentEl.appendChild(wsItem(w, w === s.workspace_path ? "📌" : "📁", w === s.workspace_path, false));
  }
}

function wsItem(path, icon, isCurrent, isFixed) {
  const item = document.createElement("div");
  item.className = "ws-item" + (isCurrent ? " ws-current" : "");
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  item.innerHTML =
    '<span class="ws-icon">' + icon + "</span>" +
    '<span class="ws-name">' + escapeHtml(isCurrent ? name + (isFixed ? "（固定）" : "（当前）") : name) + "</span>" +
    (isFixed ? "" : '<span class="sess-actions"><button class="del-btn ws-del" title="删除此工作区">' + ICON_DELETE + "</button></span>");
  item.title = path;
  if (!isCurrent) {
    item.addEventListener("click", () => switchWorkspace(path));
  }
  const del = item.querySelector(".ws-del");
  if (del) {
    del.addEventListener("click", (ev) => {
      ev.stopPropagation();
      deleteWorkspace(path);
    });
  }
  return item;
}

// 打开记忆面板（记忆块管理 + 上下文压缩/交接 + 真实水位）
async function deleteWorkspace(path) {
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  if (!confirm("删除工作区「" + name + "」？（只从列表移除，不会删除文件夹）")) return;
  try {
    const newWs = await invoke("remove_workspace", { path });
    if (newWs) {
      // 删的是当前工作区：刷新状态并重启引擎
      toast("已删除工作区，正在切换到 " + newWs + "…");
      if (state.running) await invoke("stop_engine");
      state.running = false;
      setEngineChip("off", "");
      await refreshStatus();
      renderWorkspacePanel();
      refreshFS();
      const noAuth = !!(state.settings && state.settings.no_auth);
      if (state.apiKey || (await invoke("has_api_key").catch(() => false)) || noAuth) {
        startEngine();
        setTimeout(loadSessions, 8000);
      } else {
        loadSessions();
      }
    } else {
      renderWorkspacePanel();
      toast("已删除工作区");
    }
  } catch (e) {
    toast("删除失败：" + e, "err");
  }
}

// ---------- 会话管理（dsh 式：按工作区划分的任务会话） ----------
async function switchWorkspace(path) {
  if (!path) return;
  if (path === state.workspace) { toast("已是当前工作区"); return; }
  const s = await invoke("get_settings").catch(() => null);
  if (!s) return;
  s.workspace_path = path;
  s.onboarded = true;
  try {
    await invoke("save_settings", { settings: s });
    const noAuth = s.no_auth || false;
    const hasKey = state.apiKey || (await invoke("has_api_key").catch(() => false));
    await refreshStatus();
    refreshFS();
    renderWorkspacePanel();
    // 切换/添加工作区后延迟再刷新一次文件树，确保引擎重启后仍显示新工作区文件
    setTimeout(refreshFS, 2500);
    if (hasKey || noAuth) {
      toast("已切换工作区，正在重启引擎…");
      startEngine();
      // 引擎重启后刷新会话列表
      setTimeout(loadSessions, 8000);
    } else {
      toast("工作区已切换");
    }
  } catch (e) {
    toast("切换失败：" + e, "err");
  }
}

async function pickFolderWithHint() {
  toast("请在弹出的窗口中选择一个文件夹（底部按钮为「选择文件夹」）");
  try {
    const picked = await invoke("pick_folder");
    return picked;
  } catch (e) {
    toast("选择文件夹失败：" + e, "err");
    return null;
  }
}

async function loadFS(path) {
  if (!state.workspace) return;
  const target = path || state.workspace;
  try {
    const entries = await invoke("list_dir", { path: target });
    state.fs.current = target;
    renderFS(entries);
  } catch (e) {
    state.fs.current = state.workspace;
    $("#fs-list").innerHTML = '<div class="fs-empty">' + escapeHtml(String(e)) + "</div>";
  }
}

function renderFS(entries) {
  const list = $("#fs-list");
  list.innerHTML = "";
  $("#fs-crumb").textContent = fsCrumbText(state.fs.current);
  $("#fs-crumb").title = state.fs.current;
  // 上级
  const ws = state.workspace.replace(/\\+$/, "");
  if (state.fs.current && state.fs.current !== ws && state.fs.current.length > ws.length) {
    const up = document.createElement("div");
    up.className = "fs-item fs-up";
    up.innerHTML = '<span class="fs-icon">⬆</span><span class="fs-name">上级目录…</span>';
    up.addEventListener("click", () => {
      const parent = state.fs.current.slice(0, Math.max(state.fs.current.lastIndexOf("\\"), state.fs.current.lastIndexOf("/")));
      loadFS(parent || ws);
    });
    list.appendChild(up);
  }
  if (!entries.length && !(state.fs.current && state.fs.current !== ws)) {
    const empty = document.createElement("div");
    empty.className = "fs-empty";
    empty.textContent = "（空目录）";
    list.appendChild(empty);
    return;
  }
  for (const e of entries) {
    const item = document.createElement("div");
    item.className = "fs-item" + (e.is_dir ? " fs-dir" : "");
    item.innerHTML =
      '<span class="fs-icon">' + (e.is_dir ? "📁" : "📄") + "</span>" +
      '<span class="fs-name">' + escapeHtml(e.name) + (e.is_dir ? "" : ' <span style="color:#8a94a3">' + fsFormatSize(e.size) + "</span>") + "</span>" +
      '<span class="fs-actions">' +
        (e.is_dir ? "" : '<button class="fs-btn fs-at" title="引用到对话（@）">@</button>') +
        '<button class="fs-btn" title="复制路径">⧉</button>' +
        '<button class="fs-btn" title="在文件夹中显示">📂</button>' +
      "</span>";
    const full = (state.fs.current.endsWith("\\") || state.fs.current.endsWith("/") ? state.fs.current : state.fs.current + "\\") + e.name;
    // 相对路径（供引用）
    const wsNorm = state.workspace.replace(/\\+$/, "");
    const rel = full.replace(/\\/g, "/").replace(new RegExp("^" + wsNorm.replace(/\\/g, "/").replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "/"), "");
    if (e.is_dir) {
      item.addEventListener("click", () => loadFS(full));
    } else {
      item.addEventListener("click", () => invoke("open_path", { path: full, reveal: false }).catch((err) => toast(String(err), "warn")));
    }
    const btns = item.querySelectorAll(".fs-btn");
    const atBtn = item.querySelector(".fs-at");
    if (atBtn) {
      atBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        addRef(rel);
        toast("已引用：" + rel);
      });
    }
    const copyBtn = item.querySelector('.fs-btn[title="复制路径"]');
    if (copyBtn) {
      copyBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        const copy = async () => {
          try { await navigator.clipboard.writeText(full); toast("路径已复制"); }
          catch (e2) { toast("复制失败：" + e2, "err"); }
        };
        copy();
      });
    }
    const revealBtn = item.querySelector('.fs-btn[title="在文件夹中显示"]');
    if (revealBtn) {
      revealBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        invoke("open_path", { path: full, reveal: true }).catch((err) => toast(String(err), "warn"));
      });
    }
    void btns;
    list.appendChild(item);
  }
}

async function refreshFS() {
  if (state.workspace) loadFS(state.fs.current || state.workspace);
}

// ---------- 设置 ----------