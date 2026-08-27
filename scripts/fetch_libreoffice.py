#!/usr/bin/env python3
"""下载 LibreOffice Windows x64 MSI（带重试）。"""
import os
import sys
import time
import urllib.request

VER = "26.2.5"
URL = f"https://download.documentfoundation.org/libreoffice/stable/{VER}/win/x86_64/LibreOffice_{VER}_Win_x86-64.msi"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "vendor", "libreoffice")
os.makedirs(OUT, exist_ok=True)
dest = os.path.join(OUT, os.path.basename(URL))
tmp = dest + ".part"

for attempt in range(1, 8):
    try:
        have = os.path.getsize(tmp) if os.path.exists(tmp) else 0
        headers = {"User-Agent": "Mozilla/5.0"}
        if have > 0:
            headers["Range"] = f"bytes={have}-"
        req = urllib.request.Request(URL, headers=headers)
        with urllib.request.urlopen(req, timeout=120) as resp, open(tmp, "ab") as f:
            while True:
                chunk = resp.read(1 << 20)
                if not chunk:
                    break
                f.write(chunk)
        os.replace(tmp, dest)
        print("OK", dest, os.path.getsize(dest))
        sys.exit(0)
    except Exception as e:
        print(f"RETRY {attempt}: {e}")
        time.sleep(3 * attempt)
print("FAIL")
sys.exit(1)
