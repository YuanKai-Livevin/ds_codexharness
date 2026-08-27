#!/usr/bin/env python3
"""Verify windowsSandbox/readiness with windows.sandbox = unelevated (expect ready, no UAC)."""
import json, os, subprocess, sys, tempfile, time

CODEX_HOME = os.path.join(tempfile.mkdtemp(), "codex-home")
os.makedirs(os.path.join(CODEX_HOME, "rules"), exist_ok=True)
cfg = """model = "deepseek-v4-flash"
model_provider = "deepseek"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
[windows]
sandbox = "unelevated"
[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/"
wire_api = "responses"
env_key = "OH_API_KEY"
"""
open(os.path.join(CODEX_HOME, "config.toml"), "w", encoding="utf-8").write(cfg)

env = dict(os.environ)
env["CODEX_HOME"] = CODEX_HOME
env["OH_API_KEY"] = "sk-test"
env["RUST_LOG"] = "warn"

proc = subprocess.Popen([os.path.abspath(sys.argv[1])], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)

def send(o):
    proc.stdin.write((json.dumps(o) + "\n").encode()); proc.stdin.flush()

def recv_until(pred, timeout=30):
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
        "clientInfo":{"name":"t3","title":"t","version":"0"},
        "capabilities":{"experimentalApi":True}}})
    recv_until(lambda m: m.get("id") == 1)
    send({"jsonrpc":"2.0","method":"initialized"})
    send({"jsonrpc":"2.0","id":2,"method":"windowsSandbox/readiness","params":{}})
    r = recv_until(lambda m: m.get("id") == 2)
    print("readiness (unelevated):", json.dumps(r.get("result", r), ensure_ascii=False))
    # 发起 unelevated 配置（应无 UAC，直接完成）
    send({"jsonrpc":"2.0","id":3,"method":"windowsSandbox/setupStart",
          "params":{"mode":"unelevated","cwd":os.path.abspath(".")}})
    r = recv_until(lambda m: m.get("id") == 3)
    print("setupStart:", json.dumps(r.get("result", r), ensure_ascii=False))
    # 等待 setupCompleted 通知
    n = recv_until(lambda m: m.get("method") == "windowsSandbox/setupCompleted", timeout=60)
    print("setupCompleted:", json.dumps(n.get("params", n), ensure_ascii=False))
    proc.kill()
except Exception as e:
    print("FAIL:", e); proc.kill()
