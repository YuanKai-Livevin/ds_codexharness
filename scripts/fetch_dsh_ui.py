#!/usr/bin/env python3
"""抓取 dsh web 界面的 HTML 与 CSS，提取主题样式。"""
import json
import os
import re
import urllib.request

OUT = r"F:\dshProject\codexharness\dsh-assets"
os.makedirs(OUT, exist_ok=True)

try:
    r = urllib.request.urlopen("http://127.0.0.1:3080", timeout=8)
    html = r.read().decode("utf-8", "replace")
    print("HTML status:", r.status, "len:", len(html))
    with open(os.path.join(OUT, "index.html"), "w", encoding="utf-8") as f:
        f.write(html)

    # 提取 css / js 资源
    assets = []
    for m in re.findall(r'(?:href|src)=["\']([^"\']+\.(?:css|js))["\']', html):
        assets.append(m)
    print("assets:", assets[:20])

    for a in assets:
        url = a if a.startswith("http") else "http://127.0.0.1:3080" + ("" if a.startswith("/") else "/") + a
        try:
            d = urllib.request.urlopen(url, timeout=8).read()
            name = a.split("/")[-1].split("?")[0]
            with open(os.path.join(OUT, name), "wb") as f:
                f.write(d)
            print("saved", name, len(d))
        except Exception as e:
            print("asset fail", a, e)
except Exception as e:
    print("ERR:", e)
