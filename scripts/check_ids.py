#!/usr/bin/env python3
"""Cross-check element IDs between index.html and app.js."""
import re

html = open("app/assets/index.html", encoding="utf-8").read()
js = open("app/assets/app.js", encoding="utf-8").read()

ids = set(re.findall(r'id="([^"]+)"', html))
used = set(re.findall(r'\$\(("#[^"]+")\)', js))
used = {u[1:] for u in used} | set(re.findall(r'getElementById\("([^"]+)"\)', js))

missing = used - ids
extra = ids - used
print("IDs in HTML:", len(ids))
print("Used in JS:", len(used))
print("MISSING (used in JS but not in HTML):", sorted(missing) if missing else "none")
print("Unused IDs in HTML:", sorted(extra) if extra else "none")
