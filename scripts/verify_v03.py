#!/usr/bin/env python3
"""验证 v0.3 exe 嵌入的最新前端。"""
import os

path = r"F:\dshProject\codexharness\dist\OfficeHarness-v0.3\OfficeHarness.exe"
data = open(path, "rb").read()
checks = {
    "v0.3 badge": "v0.3",
    "API Key 无效或已过期": "API Key 无效或已过期",
    "切换工作区": "切换工作区",
    "工作区文件": "工作区文件",
    "最近使用": "最近使用",
    "快捷位置": "快捷位置",
    "正在尝试自动启动": "正在尝试自动启动",
    "引擎未在运行": "引擎未在运行",
}
print(f"exe: {os.path.getsize(path)//1024} KB")
for label, needle in checks.items():
    found = needle.encode("utf-8") in data or needle.encode("utf-16-le") in data
    print(f"  {label}: {'FOUND' if found else 'not found'}")
