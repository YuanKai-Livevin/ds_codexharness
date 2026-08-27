#!/usr/bin/env python3
"""提取 dsh 完整色板：neutral-bluish 色阶 + alias 别名。"""
import re

js = open(r"F:\dshProject\codexharness\dsh-assets\index-Dqw48FrP.js", encoding="utf-8", errors="replace").read()
css = open(r"F:\dshProject\codexharness\dsh-assets\index-CSGf6Qzd.css", encoding="utf-8").read()
css2 = open(r"F:\dshProject\codexharness\dsh-assets\vendor-CjyC-hUb.css", encoding="utf-8").read()
all_css = css + "\n" + css2

print("=== neutral-bluish 色阶 ===")
found = {}
for src in (js, all_css):
    for m in re.finditer(r'--dsw-static-neutral-bluish-([\w-]+)\s*:\s*([^;}]+)', src):
        found[m.group(1)] = m.group(2).strip()
for k in sorted(found, key=lambda x: (len(x), x)):
    print(f"   bluish-{k} = {found[k]}")

print("\n=== --dsw-alias-* 别名 ===")
alias = {}
for src in (js, all_css):
    for m in re.finditer(r'--dsw-alias-([\w-]+)\s*:\s*([^;}]+)', src):
        alias[m.group(1)] = m.group(2).strip()
for k in sorted(alias):
    print(f"   alias-{k} = {alias[k]}")

print("\n=== --dsw-font-family 定义 ===")
for src in (js, all_css):
    for m in re.finditer(r'--dsw-font-family\s*:\s*([^;}]+)', src):
        print("   ", m.group(1).strip()[:200])
