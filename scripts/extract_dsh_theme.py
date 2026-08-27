#!/usr/bin/env python3
"""从 dsh 的 CSS 提取主题：变量、颜色、字体、关键布局类。"""
import re

css = open(r"F:\dshProject\codexharness\dsh-assets\index-CSGf6Qzd.css", encoding="utf-8").read()
css2 = open(r"F:\dshProject\codexharness\dsh-assets\vendor-CjyC-hUb.css", encoding="utf-8").read()
all_css = css + "\n" + css2

print("总 CSS 长度:", len(all_css))

# 1) 自定义属性定义
print("\n=== CSS 变量（:root 及 * 选择器定义）===")
for m in re.finditer(r'([^{}]*)\{([^{}]*--[^}]*)\}', all_css):
    sel, body = m.group(1).strip(), m.group(2)
    if "--" in body and ("root" in sel or "*" in sel or ":" in sel):
        vars_ = re.findall(r'(--[\w-]+)\s*:\s*([^;]+);', body)
        if vars_:
            print(sel[:60], "->", len(vars_), "vars")
            for k, v in vars_[:25]:
                print("   ", k, "=", v.strip())
            break

# 2) 常见颜色值统计
print("\n=== 高频颜色 ===")
colors = re.findall(r'#[0-9a-fA-F]{3,8}\b', all_css)
from collections import Counter
c = Counter(x.lower() for x in colors)
for color, n in c.most_common(20):
    print(f"   {color} x{n}")

# 3) 字体
print("\n=== 字体 ===")
for m in re.findall(r"font-family\s*:\s*([^;]+);", all_css)[:8]:
    print("   ", m.strip())

# 4) 关键布局类
print("\n=== 布局相关类名（sidebar/chat/main/header/footer）===")
classes = set(re.findall(r'\.([a-zA-Z][\w-]*(?:sidebar|chat|main|header|composer|input|message|conversation|session|nav|panel|toolbar|bubble|terminal)[\w-]*)', all_css, re.I))
for cls in sorted(classes)[:40]:
    print("   ." + cls)
