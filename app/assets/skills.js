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
      const item = document.createElement("div");
      item.className = "skill-item";
      item.title = sk.path;
      item.innerHTML =
        '<span class="skill-icon">🧩</span>' +
        '<div class="skill-body"><div class="skill-name">' + escapeHtml(sk.name) + "</div>" +
        (sk.description ? '<div class="skill-desc">' + escapeHtml(sk.description) + "</div>" : "") + "</div>";
      list.appendChild(item);
    }
  } catch (e) {
    list.innerHTML = '<div class="fs-empty">技能仓库读取失败：' + escapeHtml(String(e)) + "</div>";
  }
}

// ---------- 认证错误提示（Key 失效等） ----------