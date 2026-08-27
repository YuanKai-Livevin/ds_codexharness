#!/usr/bin/env python3
"""验证 thread/list 对空会话与活跃会话的过滤行为。"""
import base64, ctypes, ctypes.wintypes, json, os, subprocess, sys, tempfile, time

def dpapi_decrypt(b64: str) -> str:
    blob_in = base64.b64decode(b64)
    class DATA_BLOB(ctypes.Structure):
        _fields_ = [("cbData", ctypes.wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_ubyte))]
    in_blob = DATA_BLOB(len(blob_in), ctypes.cast(ctypes.create_string_buffer(blob_in), ctypes.POINTER(ctypes.c_ubyte)))
    out_blob = DATA_BLOB()
    ok = ctypes.windll.crypt32.CryptUnprotectData(ctypes.byref(in_blob), None, None, None, None, 0x1, ctypes.byref(out_blob))
    data = ctypes.string_at(out_blob.pbData, out_blob.cbData)
    ctypes.windll.kernel32.LocalFree(out_blob.pbData)
    return data.decode("utf-8")

settings = json.load(open(os.path.join(os.environ["APPDATA"], "OfficeHarness", "settings.json"), encoding="utf-8"))
KEY = dpapi_decrypt(settings["api_key_enc"]); WS = settings["workspace_path"]
CODEX_HOME = os.path.join(tempfile.mkdtemp(), "codex-home"); os.makedirs(CODEX_HOME, exist_ok=True)
open(os.path.join(CODEX_HOME, "config.toml"), "w", encoding="utf-8").write(f"""model = "{settings['model']}"
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
""")
env = dict(os.environ); env["CODEX_HOME"] = CODEX_HOME; env["OH_API_KEY"] = KEY; env["RUST_LOG"] = "error"
proc = subprocess.Popen([os.path.abspath(sys.argv[1])], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)

def send(o):
    proc.stdin.write((json.dumps(o) + "\n").encode()); proc.stdin.flush()
def recv_until(pred, timeout=90):
    deadline = time.time() + timeout; buf = b""
    while time.time() < deadline:
        chunk = proc.stdout.read1(1)
        if not chunk: break
        buf += chunk
        if buf.endswith(b"\n"):
            m = json.loads(buf.decode())
            if pred(m): return m
            buf = b""
    raise TimeoutError("predicate")

try:
    send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"t","title":"t","version":"0"},"capabilities":{"experimentalApi":True}}})
    recv_until(lambda m: m.get("id") == 1)
    send({"jsonrpc":"2.0","method":"initialized"})

    # 创建空会话 X（不聊天）
    send({"jsonrpc":"2.0","id":2,"method":"thread/start","params":{"cwd": WS, "sandbox":"workspace-write", "approvalPolicy":"on-request"}})
    r = recv_until(lambda m: m.get("id") == 2)
    tid_x = r["result"]["thread"]["id"]
    print("empty session X:", tid_x[:14])

    # list：看 X 是否出现
    send({"jsonrpc":"2.0","id":3,"method":"thread/list","params":{"cwd": WS, "limit": 50}})
    r = recv_until(lambda m: m.get("id") == 3)
    data = r["result"]["data"]
    print("list with empty session:", len(data), [d.get("preview","")[:10] for d in data])

    # 给 X 发一条消息
    send({"jsonrpc":"2.0","id":4,"method":"turn/start","params":{"threadId": tid_x, "input":[{"type":"text","text":"测试会话X"}]}})
    recv_until(lambda m: m.get("id") == 4)
    time.sleep(3)
    send({"jsonrpc":"2.0","id":5,"method":"thread/list","params":{"cwd": WS, "limit": 50}})
    r = recv_until(lambda m: m.get("id") == 5)
    data = r["result"]["data"]
    print("list after X chatted:", len(data), [d.get("preview","")[:10] for d in data])
    proc.kill()
except Exception as e:
    print("FAIL:", e); proc.kill()
