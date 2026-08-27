#!/usr/bin/env python3
"""确认 dsh 主题模式 + 提取品牌色/状态色。"""
import re

html = open(r"F:\dshProject\codexharness\dsh-assets\index.html", encoding="utf-8").read()
js = open(r"F:\dshProject\codexharness\dsh-assets\index-Dqw48FrP.js", encoding="utf-8", errors="replace").read()
css = open(r"F:\dshProject\codexharness\dsh-assets\index-CSGf6Qzd.css", encoding="utf-8").read()
css2 = open(r"F:\dshProject\codexharness\dsh-assets\vendor-CjyC-hUb.css", encoding="utf-8").read()
all_css = css + "\n" + css2

# 1) HTML 里的主题标记
print("=== HTML 主题标记 ===")
for m in re.findall(r'<html[^>]*>|<body[^>]*>', html)[:4]:
    print("   ", m)
print("   data-ds-dark-theme in html:", "data-ds-dark-theme" in html)

# 2) deepseek / red / green 色阶
print("\n=== deepseek 色阶 ===")
found = {}
for src in (js, all_css):
    for m in re.finditer(r'--dsw-static-deepseek-([\w-]+)\s*:\s*([^;}]+)', src):
        found[m.group(1)] = m.group(2).strip()
for k in sorted(found, key=lambda x: (len(x), x)):
    print(f"   deepseek-{k} = {found[k]}")

print("\n=== red / green / yellow / blue 色阶 ===")
for family in ("red", "green", "yellow", "blue"):
    found = {}
    pat = re.compile(r"--dsw-static-" + family + r"-([\w-]+)\s*:\s*([^;}]+)")
    for src in (js, all_css):
        for m in pat.finditer(src):
            found[m.group(1)] = m.group(2).strip()
    if found:
        print(f"  [{family}]")
        for k in sorted(found, key=lambda x: (len(x), x)):
            print(f"     {family}-{k} = {found[k]}")

# 3) 主色 deepseek-450 上下文
print("\n=== 浅色主题 alias（body 基础规则）===")
for m in re.finditer(r'body\{[^}]*--dsw-alias-[^}]*\}', all_css):
    b = m.group(0)
    if "neutral-bluish-00" in b:
        print(b[:600])
        break
