#!/usr/bin/env python3
"""验证 exe 内嵌的前端资源与版本标记。"""
import os
import sys

targets = [
    r"F:\dshProject\codexharness\target\release\office-harness.exe",
    r"F:\dshProject\codexharness\dist\OfficeHarness\OfficeHarness.exe",
]

needles = {
    "v0.3 badge": "v0.3",
    "切换工作区": "切换工作区",
    "工作区文件": "工作区文件",
    "选择文件夹": "选择文件夹",
    "快捷位置": "快捷位置",
    "自动启动": "自动启动引擎",
    "设置已保存，正在启动引擎": "设置已保存，正在启动引擎",
}

for t in targets:
    if not os.path.exists(t):
        print(t, "MISSING")
        continue
    data = open(t, "rb").read()
    print("=" * 20, t, f"({os.path.getsize(t)//1024} KB)")
    for label, n in needles.items():
        found = (n.encode("utf-8") in data) or (n.encode("utf-16-le") in data)
        print(f"  {label}: {'FOUND' if found else 'not found'}")
