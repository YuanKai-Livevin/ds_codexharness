#!/usr/bin/env python3
"""完整测试：真实 codex app-server + 真实 DeepSeek key 跑一轮 turn，抓取真实错误。"""
import json, os, subprocess, sys, tempfile, time, threading

KEY = "sk-7320f27944b74230b275281fd12326fa"
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
env["OH_API_KEY"] = KEY
env["RUST_LOG"] = "info"

proc = subprocess.Popen([os.path.abspath(sys.argv[1])], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)

stderr_lines = []
def read_stderr():
    for line in proc.stderr:
        stderr_lines.append(line.decode("utf-8", "replace").rstrip())
t = threading.Thread(target=read_stderr, daemon=True)
t.start()

def send(o):
    proc.stdin.write((json.dumps(o) + "\n").encode()); proc.stdin.flush()

def recv_until(pred, timeout=180):
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
        "clientInfo":{"name":"diag","title":"diag","version":"0"},
        "capabilities":{"experimentalApi":True}}})
    recv_until(lambda m: m.get("id") == 1)
    send({"jsonrpc":"2.0","method":"initialized"})

    send({"jsonrpc":"2.0","id":2,"method":"thread/start","params":{
        "model":"deepseek-v4-flash",
        "cwd": os.path.abspath("."),
        "sandbox":"workspace-write",
        "approvalPolicy":"on-request",
        "personality":"pragmatic",
    }})
    r = recv_until(lambda m: m.get("id") == 2)
    if "result" not in r:
        print("thread/start ERROR:", json.dumps(r, ensure_ascii=False)[:300]); proc.kill(); sys.exit(1)
    tid = r["result"]["thread"]["id"]
    print("thread:", tid[:20])

    send({"jsonrpc":"2.0","id":3,"method":"turn/start","params":{
        "threadId": tid,
        "input":[{"type":"text","text":"你好，请回复'连接成功'四个字。"}],
    }})
    r = recv_until(lambda m: m.get("id") == 3)
    print("turn/start:", "result" in r)

    # 收集事件直到 turn/completed
    deadline = time.time() + 180
    while time.time() < deadline:
        m = recv_until(lambda x: True, timeout=30)
        method = m.get("method", "")
        if method == "turn/completed":
            params = m.get("params", {})
            turn = params.get("turn", {})
            print("\n== turn/completed ==")
            print("status:", turn.get("status"))
            print("error:", json.dumps(turn.get("error"), ensure_ascii=False))
            break
        elif method == "item/agentMessage/delta":
            sys.stdout.write(m.get("params", {}).get("delta", "")); sys.stdout.flush()
        elif method == "error":
            print("\n[server error]", json.dumps(m.get("params"), ensure_ascii=False)[:300])
        elif method == "item/commandExecution/requestApproval":
            print("\n[approval?] unexpected")
    proc.kill()
    time.sleep(1)
    print("\n== stderr tail (RUST_LOG=info) ==")
    for line in stderr_lines[-40:]:
        print(" ", line[:220])
except Exception as e:
    print("FAIL:", e)
    proc.kill()
