async function renderSkills() {
  const list = $("#skills-list");
  if (!list) return;
  try {
    const info = await invoke("get_skills_repo");
    state.skillsRepo = info.path || "";
    list.innerHTML = "";
    if (!info.skills.length) {
      list.innerHTML = '<div class="fs-empty">还没有技能。点「＋」创建或导入。</div>';
      return;
    }
    for (const sk of info.skills) {
      list.appendChild(skillItem(sk));
    }
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">技能仓库读取失败：' + escapeHtml(String(e)) + "</div>";
  }
}

function trustBadge(sk) {
  if (sk.source === "builtin") return '<span class="skill-chip chip-ok">🟢 内置</span>';
  if (sk.quarantined) return '<span class="skill-chip chip-warn">🟡 隔离区·未启用</span>';
  return '<span class="skill-chip chip-warn">🟠 外部导入</span>';
}

function skillItem(sk) {
  const item = document.createElement("div");
  item.className = "skill-item" + (sk.disabled ? " skill-disabled" : "");
  item.title = sk.path + "\n目录: " + sk.dir_name + "\n全目录 SHA-256: " + sk.checksum + "…";
  const chips = [];
  chips.push('<span class="skill-chip">v' + esc(sk.version || "1.0.0") + "</span>");
  chips.push('<span class="skill-chip">👤 ' + esc(sk.author || "未知") + "</span>");
  chips.push('<span class="skill-chip" title="' + esc(sk.permissions) + '">🔒 ' + esc((sk.permissions || "工作区内读写").slice(0, 12)) + "</span>");
  chips.push('<span class="skill-chip" title="全目录 SHA-256（含全部脚本与资源）">#' + esc((sk.checksum || "").slice(0, 8)) + "</span>");
  chips.push('<span class="skill-chip" title="文件数">📄 ' + sk.file_count + " 文件</span>");
  chips.push(trustBadge(sk));
  if (sk.disabled) chips.push('<span class="skill-chip chip-off">已禁用</span>');
  if (sk.has_selftest) chips.push('<span class="skill-chip">🧪 有测试</span>');
  item.innerHTML =
    '<span class="skill-icon">' + (sk.disabled ? "⏸" : "🧩") + "</span>" +
    '<div class="skill-body">' +
      '<div class="skill-name">' + esc(sk.name) + "</div>" +
      (sk.description ? '<div class="skill-desc">' + esc(sk.description) + "</div>" : "") +
      '<div class="skill-chips">' + chips.join("") + "</div>" +
      '<div class="skill-btns">' +
        (sk.disabled
          ? '<button class="btn btn-small btn-primary" data-act="enable">启用</button>'
          : '<button class="btn btn-small" data-act="disable">禁用</button>') +
        '<button class="btn btn-small" data-act="test">🧪 测试</button>' +
        (sk.has_backup ? '<button class="btn btn-small btn-ghost" data-act="rollback">↩ 回滚</button>' : "") +
        '<button class="btn btn-small btn-ghost" data-act="open">打开目录</button>' +
      "</div>" +
    "</div>";
  item.querySelector('[data-act="open"]').addEventListener("click", async (e) => {
    e.stopPropagation();
    try { await invoke("open_path", { path: sk.path, reveal: true }); }
    catch (err) { toast("打开失败：" + err, "err"); }
  });
  item.querySelector('[data-act="' + (sk.disabled ? "enable" : "disable") + '"]').addEventListener("click", async (e) => {
    e.stopPropagation();
    try {
      if (sk.disabled) {
        await invoke("enable_skill", { name: sk.name });
        toast("已启用技能：" + sk.name + "（外部技能启用后进入模型视野，测试仍需确认）", "ok");
      } else {
        await invoke("disable_skill", { name: sk.name });
        toast("已禁用技能：" + sk.name + "（模型将不再看到它，可随时恢复）", "ok");
      }
      renderSkills();
    } catch (err) { toast("操作失败：" + err, "err"); }
  });
  item.querySelector('[data-act="test"]').addEventListener("click", async (e) => {
    e.stopPropagation();
    await runSkillTest(item, sk);
  });
  if (sk.has_backup) {
    item.querySelector('[data-act="rollback"]').addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!confirm("回滚「" + sk.name + "」到最近一次导入前的版本？当前版本将被替换。")) return;
      try {
        await invoke("rollback_skill", { name: sk.name });
        toast("已回滚技能：" + sk.name, "ok");
        renderSkills();
      } catch (err) { toast("回滚失败：" + err, "err"); }
    });
  }
  return item;
}

// 测试技能：外部技能先预览脚本确认，再隔离运行
async function runSkillTest(item, sk) {
  const btn = item.querySelector('[data-act="test"]');
  btn.disabled = true;
  btn.textContent = "测试中…";
  try {
    // 第一步：获取（外部技能会返回 need_confirm + 脚本预览）
    let r = await invoke("test_skill", { name: sk.name, confirmed: false });
    if (r.need_confirm) {
      const ok = await confirmSkillScript(sk, r.script_preview);
      if (!ok) { toast("已取消测试"); return; }
      r = await invoke("test_skill", { name: sk.name, confirmed: true });
    }
    if (!r.ran) {
      toast(r.output_tail, "warn");
      return;
    }
    const pass = /(\d+)\s*通过/.exec(r.output_tail);
    let msg = "测试" + (r.exit_code === 0 ? "通过 ✅" : "失败 ⚠️") +
      (pass ? "（" + pass[1] + " 项通过）" : "");
    if (r.timed_out) msg = "⏱ 测试超时已终止（进程树已清理）";
    if (r.sandboxed) msg += " · 隔离运行";
    toast(msg, r.exit_code === 0 ? "ok" : "err");
    showSkillTestOutput(item, r.output_tail, r.exit_code, r.sandboxed, r.timed_out);
  } catch (err) {
    toast("测试失败：" + err, "err");
  } finally {
    btn.disabled = false;
    btn.textContent = "🧪 测试";
  }
}

// 外部技能测试确认弹窗：展示将要执行的脚本内容
function confirmSkillScript(sk, preview) {
  return new Promise((resolve) => {
    const mask = document.createElement("div");
    mask.className = "modal-mask";
    mask.style.zIndex = "80";
    const card = document.createElement("div");
    card.className = "modal";
    card.style.width = "620px";
    card.innerHTML =
      "<h2>⚠ 外部技能测试确认</h2>" +
      '<p class="hint">技能「' + esc(sk.name) + '」来自外部导入，以下测试脚本将以<strong>当前用户权限</strong>执行：' +
      "将在<strong>隔离副本</strong>中运行（无 API 凭据、30 秒超时、超时后终止整个进程树）。" +
      "请先检查脚本内容，确认无恶意行为后再运行。</p>" +
      '<pre class="code-block" style="max-height:260px;overflow-y:auto;white-space:pre-wrap;font-size:11px">' + esc(preview || "（无脚本内容）") + "</pre>" +
      '<div class="modal-actions">' +
      '<button class="btn btn-ghost" id="sk-confirm-no">取消</button>' +
      '<button class="btn btn-primary" id="sk-confirm-yes">确认隔离运行</button>' +
      "</div>";
    mask.appendChild(card);
    document.body.appendChild(mask);
    mask.querySelector("#sk-confirm-yes").addEventListener("click", () => { mask.remove(); resolve(true); });
    mask.querySelector("#sk-confirm-no").addEventListener("click", () => { mask.remove(); resolve(false); });
  });
}

function showSkillTestOutput(item, tail, exitCode, sandboxed, timedOut) {
  const old = item.querySelector(".skill-test-out");
  if (old) old.remove();
  const pre = document.createElement("pre");
  pre.className = "skill-test-out";
  const meta = [];
  if (sandboxed) meta.push("隔离运行");
  if (timedOut) meta.push("超时终止");
  pre.textContent = (meta.length ? "[" + meta.join(" · ") + "] " : "") +
    "退出码: " + (exitCode == null ? "—" : exitCode) + "\n" + (tail || "（无输出）");
  item.querySelector(".skill-body").appendChild(pre);
  scrollBottom();
}

// ---------- 认证错误提示（Key 失效等） ----------
