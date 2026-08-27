#!/usr/bin/env python3
"""用 ASCII 标记验证最新 app.js / index.html 是否嵌入 v0.3 exe。"""
import os

path = r"F:\dshProject\codexharness\dist\OfficeHarness-v0.3\OfficeHarness.exe"
data = open(path, "rb").read()

# 最新 app.js 中的独有函数名（ASCII）
checks = {
    "renderWorkspacePanel": b"renderWorkspacePanel",
    "pickFolderWithHint": b"pickFolderWithHint",
    "handleEngineError": b"handleEngineError",
    "switchWorkspace": b"switchWorkspace",
    "fsCrumbText": b"fsCrumbText",
    "sandboxSetupResult": b"sandboxSetupResult",
    "winsandbox": b"winsandbox",
    "common_folders": b"common_folders",
    "topbar-ws": b"topbar-ws",
    "set-winsandbox": b"set-winsandbox",
}
all_found = True
for label, needle in checks.items():
    found = needle in data
    if not found:
        all_found = False
    print(f"  {label}: {'FOUND' if found else 'NOT FOUND'}")
print("ALL LATEST JS MARKERS PRESENT" if all_found else "!!! SOME MARKERS MISSING - OLD FRONTEND EMBEDDED !!!")
