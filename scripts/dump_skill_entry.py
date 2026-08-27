#!/usr/bin/env python3
"""Dump raw skills/list entry structure."""
import json

import sys

sys.path.insert(0, "scripts")

# 复用 verify_skills 的启动逻辑，只改打印
src = open("scripts/verify_skills.py", encoding="utf-8").read()
src = src.replace(
    'for d in data[:10]:\n        print("  -", d.get("name"), "|", str(d.get("description",""))[:50])',
    'print("RAW:", json.dumps(data[0], ensure_ascii=False)[:600])',
)
exec(src)
