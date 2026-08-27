#!/usr/bin/env python3
"""检查 LibreOffice 下载源与最新版本。"""
import re
import ssl
import urllib.request

ctx = ssl.create_default_context()
try:
    r = urllib.request.urlopen("https://download.documentfoundation.org/libreoffice/stable/", timeout=20, context=ctx)
    print("stable dir status:", r.status)
    html = r.read().decode("utf-8", "replace")
    vers = re.findall(r'href="([\d]+\.[\d]+\.[\d]+)/"', html)
    print("versions found:", vers)
    if vers:
        latest = vers[-1]
        print("latest:", latest)
        # 检查 win x64 MSI
        msi_url = f"https://download.documentfoundation.org/libreoffice/stable/{latest}/win/x86_64/LibreOffice_{latest}_Win_x86-64.msi"
        req = urllib.request.Request(msi_url, method="HEAD")
        try:
            rr = urllib.request.urlopen(req, timeout=20, context=ctx)
            print("MSI HEAD:", rr.status, "size:", rr.headers.get("Content-Length"))
        except Exception as e:
            print("MSI HEAD err:", e)
except Exception as e:
    print("ERR:", e)
