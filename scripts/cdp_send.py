#!/usr/bin/env python3
"""CDP 端到端：在真实应用里发消息并等待回复。"""
import json
import sys
import time
import urllib.request
import websocket

targets = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json/list", timeout=5))
page = next((t for t in targets if t.get("type") == "page"), targets[0])
ws = websocket.create_connection(page["webSocketDebuggerUrl"], timeout=15, origin=None)
mid = 0

def evaluate(expr):
    global mid
    mid += 1
    ws.send(json.dumps({"id": mid, "method": "Runtime.evaluate", "params": {"expression": expr, "returnByValue": True}}))
    while True:
        r = json.loads(ws.recv())
        if r.get("id") == mid:
            res = r.get("result", {}).get("result", {})
            if res.get("subtype") == "error":
                return "EXC:" + res.get("description", "")
            return res.get("value")

print("chip:", evaluate("document.getElementById('engine-chip').textContent"))
print("send disabled:", evaluate("document.getElementById('btn-send').disabled"))
print("fatal:", evaluate("var b=document.getElementById('fatal-bar'); b?(b.textContent||''):''"))

# 注入消息并发送
print("set composer:", evaluate("document.getElementById('composer').value = '你好，请只回复四个字：连接成功。'"))
print("trigger send:", evaluate("send()"))
print("sent.")

# 等待回复
for i in range(40):
    time.sleep(1)
    n = evaluate("document.getElementById('messages').children.length")
    last = evaluate("var m=document.getElementById('messages'); m.lastElementChild ? m.lastElementChild.innerText : ''")
    print(f"  [{i+1}s] children={n} last={last[:60]!r}")
    if "连接成功" in str(last):
        print("REPLY OK")
        break
    if i >= 15 and n >= 2:
        # 已发送但可能还在等，继续
        pass
ws.close()
