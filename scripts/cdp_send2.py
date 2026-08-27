#!/usr/bin/env python3
"""CDP 精确检查：发消息后检查 messages 里是否有助手回复气泡。"""
import json
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

# 发一条新消息
evaluate("document.getElementById('composer').value = '请用一句话介绍你自己。'")
evaluate("send()")
print("sent, waiting...")

for i in range(30):
    time.sleep(1)
    detail = evaluate("""(function(){
      var m = document.getElementById('messages');
      var out = [];
      for (var i = 0; i < m.children.length; i++) {
        out.push(m.children[i].className + ' :: ' + (m.children[i].innerText || '').slice(0, 80));
      }
      return JSON.stringify(out);
    })()""")
    try:
        items = json.loads(detail)
        print(f"  [{i+1}s] {len(items)} msgs")
        for it in items:
            print("     ", it)
        if len(items) >= 2 and "assistant" in str(items[-1]) or (len(items) >= 2 and "assistant" in str(items[-2] if len(items) > 1 else "")):
            # 有 assistant 气泡即认为回复已渲染
            if len(items) >= 2 and ("assistant" in items[-1] or "assistant" in items[-2]):
                print("REPLY RENDERED OK")
                break
    except Exception as ex:
        print("  parse err", ex)
ws.close()
