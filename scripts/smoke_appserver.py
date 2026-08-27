#!/usr/bin/env python3
"""Smoke test: drive codex-app-server over stdio JSON-RPC (initialize only)."""
import json
import os
import subprocess
import sys
import tempfile
import time

CODEX_HOME = os.path.join(tempfile.mkdtemp(), "codex-home")
os.makedirs(CODEX_HOME, exist_ok=True)
cfg = """model = "deepseek-v4-flash"
model_provider = "deepseek"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/"
wire_api = "responses"
env_key = "OH_API_KEY"
"""
with open(os.path.join(CODEX_HOME, "config.toml"), "w", encoding="utf-8") as f:
    f.write(cfg)

env = dict(os.environ)
env["CODEX_HOME"] = CODEX_HOME
env["OH_API_KEY"] = "sk-test-fake-key"
env["RUST_LOG"] = "warn"

app_server = os.path.abspath(sys.argv[1])
proc = subprocess.Popen(
    [app_server], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env,
)


def send(obj):
    proc.stdin.write((json.dumps(obj) + "\n").encode("utf-8"))
    proc.stdin.flush()


def recv(timeout=30):
    deadline = time.time() + timeout
    buf = b""
    while time.time() < deadline:
        chunk = proc.stdout.read1(1)
        if not chunk:
            break
        buf += chunk
        if buf.endswith(b"\n"):
            return json.loads(buf.decode("utf-8"))
    raise TimeoutError(f"no complete line within {timeout}s; got {buf!r}")


try:
    # initialize
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
          "params": {"clientInfo": {"name": "smoke", "title": "smoke", "version": "0.0.1"},
                     "capabilities": {"experimentalApi": True}}})
    r = recv()
    print("initialize result keys:", sorted(r.get("result", {}).keys()) if "result" in r else r)
    send({"jsonrpc": "2.0", "method": "initialized"})

    # thread/start
    send({"jsonrpc": "2.0", "id": 2, "method": "thread/start", "params": {
        "model": "deepseek-v4-flash",
        "cwd": os.path.abspath("."),
        "sandbox": "workspace-write",
        "approvalPolicy": "on-request",
        "personality": "pragmatic",
        "developerInstructions": "你是办公助手。",
    }})
    r = recv()
    while "id" not in r or r.get("id") != 2:
        print("  (skip notification:", r.get("method", "?"), ")")
        r = recv()
    if "result" in r:
        tid = r["result"]["thread"]["id"]
        print("thread/start OK, thread id:", tid)
        # turn/start with text input
        send({"jsonrpc": "2.0", "id": 3, "method": "turn/start", "params": {
            "threadId": tid,
            "input": [{"type": "text", "text": "你好"}],
        }})
        r = recv()
        while "id" not in r or r.get("id") != 3:
            print("  (skip notification:", r.get("method", "?"), ")")
            r = recv()
        print("turn/start:", "result" in r, r.get("result", {}).get("turn", {}).get("id", "")[:20])
    else:
        print("thread/start ERROR:", json.dumps(r, ensure_ascii=False)[:500])
    # drain a few notifications
    time.sleep(2)
    proc.kill()
except Exception as e:
    print("FAIL:", e)
    proc.kill()
