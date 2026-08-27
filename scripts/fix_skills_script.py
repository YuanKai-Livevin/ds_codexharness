#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""修复 install_skills.py 中内嵌脚本的 docstring 与外层字符串冲突。"""
p = "scripts/install_skills.py"
s = open(p, encoding="utf-8").read()

replacements = [
    ('"""合并多个 Excel 文件。用法见 SKILL.md。"""', '# 合并多个 Excel 文件。用法见 SKILL.md。'),
    ('"""Excel/CSV 数据分析与分组汇总。"""', '# Excel/CSV 数据分析与分组汇总。'),
    ('"""PDF 工具：merge / split / text / info。"""', '# PDF 工具：merge / split / text / info。'),
    ('"""Excel/CSV -> 数据汇报 PPT。"""', '# Excel/CSV -> 数据汇报 PPT。'),
    ('"""图片批量处理。"""', '# 图片批量处理。'),
    ('"""数据清洗。"""', '# 数据清洗。'),
    ('"""zip 打包/解压/查看。"""', '# zip 打包/解压/查看。'),
]
for a, b in replacements:
    if a in s:
        s = s.replace(a, b)
        print("replaced:", a[:40])
    else:
        print("NOT FOUND:", a[:40])

# fill_word.py 的多行 docstring
start = '"""Word 占位符模板批量填充：'
if start in s:
    i = s.index(start)
    j = s.index('"""', i + 3)
    s = s[:i] + "# Word 占位符模板批量填充（用法见 SKILL.md）" + s[j + 3 :]
    print("replaced multi-line fill_word docstring")

open(p, "w", encoding="utf-8").write(s)
print("done")
