#!/usr/bin/env python3
"""解密 settings.json 中的 API Key 并测试有效性。"""
import base64
import ctypes
import ctypes.wintypes
import json
import os
import urllib.request


def dpapi_decrypt(b64: str) -> str:
    blob_in = base64.b64decode(b64)
    in_len = len(blob_in)
    class DATA_BLOB(ctypes.Structure):
        _fields_ = [("cbData", ctypes.wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_ubyte))]
    in_blob = DATA_BLOB(in_len, ctypes.cast(ctypes.create_string_buffer(blob_in), ctypes.POINTER(ctypes.c_ubyte)))
    out_blob = DATA_BLOB()
    ok = ctypes.windll.crypt32.CryptUnprotectData(
        ctypes.byref(in_blob), None, None, None, None, 0x1, ctypes.byref(out_blob))
    if not ok:
        raise RuntimeError("decrypt failed")
    data = ctypes.string_at(out_blob.pbData, out_blob.cbData)
    ctypes.windll.kernel32.LocalFree(out_blob.pbData)
    return data.decode("utf-8")


path = os.path.join(os.environ["APPDATA"], "OfficeHarness", "settings.json")
s = json.load(open(path, encoding="utf-8"))
if not s.get("api_key_enc"):
    print("settings.json 里没有保存的 Key")
    exit(0)
key = dpapi_decrypt(s["api_key_enc"])
print("stored key (masked):", key[:6] + "****" + key[-4:])

req = urllib.request.Request("https://api.deepseek.com/models", headers={
    "Authorization": "Bearer " + key, "Content-Type": "application/json"})
try:
    with urllib.request.urlopen(req, timeout=25) as resp:
        print("models API:", resp.status, "-> Key 有效 ✓")
except urllib.error.HTTPError as e:
    print("models API:", e.code, "->", e.read(200).decode("utf-8", "replace"))
except Exception as e:
    print("ERR:", e)
