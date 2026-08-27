#!/usr/bin/env python3
"""验证会话管理 API 全流程：thread/start、turn、list、resume、read、delete。"""
import base64
import ctypes
import ctypes.wintypes
import json
import os
import subprocess
import sys
import tempfile
import time

def dpapi_decrypt(b64: str) -> str:
    blob_in = base64.b64decode(b64)
    class DATA_BLOB(ctypes.Structure):
        _fields_ = [("cbData", ctypes.wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_ubyte))]
    in_blob = DATA_BLOB(len(blob_in), ctypes.cast(ctypes.create_string_buffer(blob_in), ctypes.POINTER(ctypes.c_ubyte)))
    out_blob = DATA_BLOB()
    ok = ctypes.windll.crypt32.CryptUnprotectData(ctypes.byref(in_blob), None, None, None, None, 0x1, ctypes.byref(out_blob))
    if not ok:
        raise RuntimeError("decrypt failed")
    data = ctypes.string_at(out_blob.pbData, out_blob.cbData)
    ctypes.windll.kernel32.LocalFree(out_blob.pbData)
    return data.decode("utf-8")

settings = json.load(open(os.path.join(os.environ["APPDATA"], "OfficeHarness", "settings.json"), encoding="utf-8"))
KEY = dpapi_decrypt(settings["api_key_enc"])
WS = settings["workspace_path"]

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

def recv_until(pred, timeout=90):
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
    raise TimeoutError("predicate not met")

try:
    send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "clientInfo":{"name":"diag","title":"t","version":"0"},"capabilities":{"experimentalApi":True}}})
    recv_until(lambda m: m.get("id") == 1)
    send({"jsonrpc":"2.0","method":"initialized"})

    # 会话 A
    send({"jsonrpc":"2.0","id":2,"method":"thread/start","params":{"cwd": WS, "sandbox":"workspace-write", "approvalPolicy":"on-request"}})
    r = recv_until(lambda m: m.get("id") == 2)
    tid_a = r["result"]["thread"]["id"]
    print("session A:", tid_a[:16])
    # 发一条消息
    send({"jsonrpc":"2.0","id":3,"method":"turn/start","params":{"threadId": tid_a, "input":[{"type":"text","text":"你好"}]}})
    recv_until(lambda m: m.get("id") == 3)
    time.sleep(3)  # 等 turn 完成写入 rollout

    # 会话 B
    send({"jsonrpc":"2.0","id":4,"method":"thread/start","params":{"cwd": WS, "sandbox":"workspace-write", "approvalPolicy":"on-request"}})
    r = recv_until(lambda m: m.get("id") == 4)
    tid_b = r["result"]["thread"]["id"]
    print("session B:", tid_b[:16])

    # list
    send({"jsonrpc":"2.0","id":5,"method":"thread/list","params":{"cwd": WS, "limit": 20}})
    r = recv_until(lambda m: m.get("id") == 5)
    data = r["result"]["data"]
    print("list:", len(data), "sessions, previews:", [d.get("preview","")[:12] for d in data])

    # resume A
    send({"jsonrpc":"2.0","id":6,"method":"thread/resume","params":{"threadId": tid_a}})
    r = recv_until(lambda m: m.get("id") == 6)
    print("resume A:", "OK" if "result" in r else r)

    # read A history
    send({"jsonrpc":"2.0","id":7,"method":"thread/read","params":{"threadId": tid_a, "includeTurns": True}})
    r = recv_until(lambda m: m.get("id") == 7)
    turns = r.get("result", {}).get("thread", {}).get("turns", [])
    msgs = []
    for t in turns:
        for it in t.get("items", []):
            if it.get("type") in ("userMessage", "agentMessage") and it.get("text"):
                msgs.append((it["type"], it["text"][:20]))
    print("history A items:", len(msgs), msgs[:4])

    # delete B
    send({"jsonrpc":"2.0","id":8,"method":"thread/delete","params":{"threadId": tid_b}})
    r = recv_until(lambda m: m.get("id") == 8)
    print("delete B:", "OK" if "result" in r else r)

    # list again
    send({"jsonrpc":"2.0","id":9,"method":"thread/list","params":{"cwd": WS, "limit": 20}})
    r = recv_until(lambda m: m.get("id") == 9)
    print("list after delete:", len(r["result"]["data"]), "sessions")
    proc.kill()
except Exception as e:
    print("FAIL:", e); proc.kill()
