#!/usr/bin/env python3
"""验证 skills/extraRoots/set + skills/list 注册与识别。"""
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
SKILLS = os.path.join(os.environ["APPDATA"], "OfficeHarness", "skills")

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
    # 注册技能根
    send({"jsonrpc":"2.0","id":2,"method":"skills/extraRoots/set","params":{"extraRoots":[SKILLS]}})
    r = recv_until(lambda m: m.get("id") == 2)
    print("extraRoots/set:", "OK" if "result" in r else r)
    # 列技能
    send({"jsonrpc":"2.0","id":3,"method":"skills/list","params":{"cwds":[WS],"forceReload":True}})
    r = recv_until(lambda m: m.get("id") == 3)
    data = r.get("result", {}).get("data", [])
    print("skills found:", len(data))
    for d in data[:10]:
        print("  -", d.get("name"), "|", str(d.get("description",""))[:50])
    proc.kill()
except Exception as e:
    print("FAIL:", e); proc.kill()
