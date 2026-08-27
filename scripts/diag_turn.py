#!/usr/bin/env python3
"""用 settings.json 里保存的真实 Key 跑一轮完整 turn 诊断。"""
import base64
import ctypes
import ctypes.wintypes
import json
import os
import subprocess
import sys
import tempfile
import threading
import time

# --- DPAPI decrypt ---
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

settings = json.load(open(os.path.join(os.environ["APPDATA"], "OfficeHarness", "settings.json"), encoding="utf-8"))
KEY = dpapi_decrypt(settings["api_key_enc"])
WS = settings["workspace_path"]
print("key (masked):", KEY[:6] + "****" + KEY[-4:])
print("workspace:", WS)

CODEX_HOME = os.path.join(tempfile.mkdtemp(), "codex-home")
os.makedirs(os.path.join(CODEX_HOME, "rules"), exist_ok=True)
cfg = f"""model = "{settings['model']}"
model_provider = "{settings['provider_name']}"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
[windows]
sandbox = "unelevated"
[model_providers.{settings['provider_name']}]
name = "DeepSeek"
base_url = "{settings['base_url']}"
wire_api = "responses"
env_key = "OH_API_KEY"
"""
open(os.path.join(CODEX_HOME, "config.toml"), "w", encoding="utf-8").write(cfg)

env = dict(os.environ)
env["CODEX_HOME"] = CODEX_HOME
env["OH_API_KEY"] = KEY
env["RUST_LOG"] = "error"

proc = subprocess.Popen([os.path.abspath(sys.argv[1])], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)

def send(o):
    proc.stdin.write((json.dumps(o) + "\n").encode()); proc.stdin.flush()

def recv_until(pred, timeout=120):
    deadline = time.time() + timeout
    buf = b""
    while time.time() < deadline:
        chunk = proc.stdout.read1(1)
        if not chunk: break
        buf += chunk
        if buf.endswith(b"\n"):
            m = json.loads(buf.decode())
            if pred(m): return m
            buf = b""
    raise TimeoutError(f"predicate not met within {timeout}s")

try:
    send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "clientInfo":{"name":"diag2","title":"t","version":"0"},"capabilities":{"experimentalApi":True}}})
    recv_until(lambda m: m.get("id") == 1)
    send({"jsonrpc":"2.0","method":"initialized"})
    send({"jsonrpc":"2.0","id":2,"method":"thread/start","params":{
        "model": settings["model"], "cwd": WS, "sandbox":"workspace-write",
        "approvalPolicy":"on-request", "personality":"pragmatic"}})
    r = recv_until(lambda m: m.get("id") == 2)
    if "result" not in r:
        print("thread/start ERROR:", json.dumps(r, ensure_ascii=False)[:300]); proc.kill(); sys.exit(1)
    tid = r["result"]["thread"]["id"]
    print("thread ok:", tid[:16])
    send({"jsonrpc":"2.0","id":3,"method":"turn/start","params":{
        "threadId": tid, "input":[{"type":"text","text":"你好，请只回复四个字：连接成功。"}]}})
    r = recv_until(lambda m: m.get("id") == 3)
    print("turn/start:", "result" in r)
    # 收集到 turn/completed
    deadline = time.time() + 120
    while time.time() < deadline:
        m = recv_until(lambda x: True, timeout=30)
        method = m.get("method", "")
        if method == "turn/completed":
            turn = m.get("params", {}).get("turn", {})
            print("turn status:", turn.get("status"))
            print("turn error:", json.dumps(turn.get("error"), ensure_ascii=False)[:200] if turn.get("error") else None)
            break
        elif method == "item/agentMessage/delta":
            sys.stdout.write(m.get("params", {}).get("delta", "")); sys.stdout.flush()
        elif method == "error":
            print("\n[server error]", json.dumps(m.get("params", {}).get("error", {}), ensure_ascii=False)[:200])
    proc.kill()
except Exception as e:
    print("FAIL:", e); proc.kill()
