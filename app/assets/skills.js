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

function skillItem(sk) {
  const item = document.createElement("div");
  item.className = "skill-item" + (sk.disabled ? " skill-disabled" : "");
  item.title = sk.path;
  const chips = [];
  chips.push('<span class="skill-chip">v' + esc(sk.version || "1.0.0") + "</span>");
  chips.push('<span class="skill-chip">👤 ' + esc(sk.author || "未知") + "</span>");
  chips.push('<span class="skill-chip" title="' + esc(sk.permissions) + '">🔒 ' + esc((sk.permissions || "工作区内读写").slice(0, 12)) + "</span>");
  chips.push('<span class="skill-chip" title="SKILL.md 校验和：' + esc(sk.checksum) + '">#' + esc((sk.checksum || "").slice(0, 8)) + "</span>");
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
        toast("已启用技能：" + sk.name, "ok");
      } else {
        await invoke("disable_skill", { name: sk.name });
        toast("已禁用技能：" + sk.name + "（模型将不再看到它，可随时恢复）", "ok");
      }
      renderSkills();
    } catch (err) { toast("操作失败：" + err, "err"); }
  });
  if (sk.has_selftest) {
    item.querySelector('[data-act="test"]').addEventListener("click", async (e) => {
      e.stopPropagation();
      const btn = e.target;
      btn.disabled = true;
      btn.textContent = "测试中…";
      try {
        const r = await invoke("test_skill", { name: sk.name });
        if (!r.ran) {
          toast(r.output_tail, "warn");
        } else {
          const pass = /(\d+)\s*通过/.exec(r.output_tail);
          toast("测试" + (r.exit_code === 0 ? "通过 ✅" : "失败 ⚠️") +
            (pass ? "（" + pass[1] + " 项通过）" : ""), r.exit_code === 0 ? "ok" : "err");
          showSkillTestOutput(item, r.output_tail, r.exit_code);
        }
      } catch (err) {
        toast("测试失败：" + err, "err");
      } finally {
        btn.disabled = false;
        btn.textContent = "🧪 测试";
      }
    });
  } else {
    const t = item.querySelector('[data-act="test"]');
    t.addEventListener("click", (e) => {
      e.stopPropagation();
      toast("该技能没有内置测试脚本（selftest.py / otools.py）", "warn");
    });
  }
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

function showSkillTestOutput(item, tail, exitCode) {
  const old = item.querySelector(".skill-test-out");
  if (old) old.remove();
  const pre = document.createElement("pre");
  pre.className = "skill-test-out";
  pre.textContent = "退出码: " + (exitCode == null ? "—" : exitCode) + "\n" + (tail || "（无输出）");
  item.querySelector(".skill-body").appendChild(pre);
  scrollBottom();
}

// ---------- 认证错误提示（Key 失效等） ----------
