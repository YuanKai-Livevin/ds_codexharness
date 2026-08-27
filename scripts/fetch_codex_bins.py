#!/usr/bin/env python3
"""Robust downloader with retries + resume for codex Windows binaries."""
import os
import sys
import time
import urllib.request

BASE = "https://github.com/openai/codex/releases/download/rust-v0.149.0/"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "vendor", "codex-bin")

FILES = [
    "codex-x86_64-pc-windows-msvc.exe.zip",
    "codex-app-server-x86_64-pc-windows-msvc.exe.zip",
    "codex-windows-sandbox-setup-x86_64-pc-windows-msvc.exe.zip",
    "codex-command-runner-x86_64-pc-windows-msvc.exe.zip",
]


def download(name: str, max_retries: int = 8):
    os.makedirs(OUT, exist_ok=True)
    dest = os.path.join(OUT, name)
    tmp = dest + ".part"
    url = BASE + name
    for attempt in range(1, max_retries + 1):
        try:
            have = os.path.getsize(tmp) if os.path.exists(tmp) else 0
            headers = {"User-Agent": "Mozilla/5.0", "Accept-Encoding": "identity"}
            if have > 0:
                headers["Range"] = f"bytes={have}-"
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=120) as resp, open(tmp, "ab") as f:
                total = resp.headers.get("Content-Length")
                while True:
                    chunk = resp.read(1 << 20)
                    if not chunk:
                        break
                    f.write(chunk)
                    have += len(chunk)
            # validate: if server sent Content-Range, expected length is in Content-Range
            os.replace(tmp, dest)
            size = os.path.getsize(dest)
            print(f"OK  {name} {size} bytes")
            return True
        except Exception as e:
            print(f"RETRY {name} attempt {attempt}: {e}")
            time.sleep(2 * attempt)
    print(f"FAIL {name}")
    return False


def main():
    names = sys.argv[1:] or FILES
    ok = True
    for n in names:
        ok = download(n) and ok
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
