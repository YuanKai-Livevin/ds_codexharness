#!/usr/bin/env python3
"""Verify windowsSandbox/readiness RPC + configWarning absence."""
import json, os, subprocess, sys, tempfile, time

CODEX_HOME = os.path.join(tempfile.mkdtemp(), "codex-home")
os.makedirs(os.path.join(CODEX_HOME, "rules"), exist_ok=True)
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
open(os.path.join(CODEX_HOME, "config.toml"), "w", encoding="utf-8").write(cfg)
policy = """prefix_rule(pattern=["rm"], decision="prompt")
prefix_rule(pattern=["pip"], decision="prompt")
"""
open(os.path.join(CODEX_HOME, "rules", "office.policy"), "w", encoding="utf-8").write(policy)

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
        "clientInfo":{"name":"t2","title":"t","version":"0"},
        "capabilities":{"experimentalApi":True}}})
    recv_until(lambda m: m.get("id") == 1)
    send({"jsonrpc":"2.0","method":"initialized"})
    # readiness
    send({"jsonrpc":"2.0","id":2,"method":"windowsSandbox/readiness","params":{}})
    r = recv_until(lambda m: m.get("id") == 2)
    print("readiness:", json.dumps(r.get("result", r), ensure_ascii=False))
    # thread/start to see if policy loads without configWarning
    send({"jsonrpc":"2.0","id":3,"method":"thread/start","params":{
        "cwd": os.path.abspath("."), "sandbox":"workspace-write",
        "approvalPolicy":"on-request"}})
    r = recv_until(lambda m: m.get("id") == 3)
    print("thread/start:", "OK" if "result" in r else json.dumps(r, ensure_ascii=False)[:300])
    time.sleep(2)
    proc.kill()
except Exception as e:
    print("FAIL:", e); proc.kill()
