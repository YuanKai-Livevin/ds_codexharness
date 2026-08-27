#!/usr/bin/env python3
"""深入提取 dsh 主题：--ds-* 变量、body 背景、关键组件样式。"""
import re

js = open(r"F:\dshProject\codexharness\dsh-assets\index-Dqw48FrP.js", encoding="utf-8", errors="replace").read()
css = open(r"F:\dshProject\codexharness\dsh-assets\index-CSGf6Qzd.css", encoding="utf-8").read()
css2 = open(r"F:\dshProject\codexharness\dsh-assets\vendor-CjyC-hUb.css", encoding="utf-8").read()
all_css = css + "\n" + css2

print("=== --ds-* 变量定义 ===")
found = set()
for m in re.finditer(r'--ds-([\w-]+)\s*:\s*([^;}]+)', js):
    found.add((m.group(1), m.group(2).strip()))
for m in re.finditer(r'--ds-([\w-]+)\s*:\s*([^;}]+)', all_css):
    found.add((m.group(1), m.group(2).strip()))
for k, v in sorted(found)[:60]:
    print(f"   --ds-{k} = {v}")

print("\n=== body / html 规则 ===")
for m in re.finditer(r'(html|body)[^{]*\{([^}]*)\}', all_css):
    sel, body = m.group(0)[:60], m.group(2)
    if "background" in body or "color" in body or "font" in body:
        print(sel[:50], "->", body.strip()[:200])

print("\n=== 背景色上下文 ===")
for color in ["#0f1115", "#f9fafb", "#f5f6f7", "#3964fe", "#99c8ff"]:
    idx = all_css.find(color)
    if idx >= 0:
        ctx = all_css[max(0, idx-80):idx+80].replace("\n", " ")
        print(f"\n[{color}] ...{ctx}...")
