#!/usr/bin/env python3
"""等待并检查最新回复渲染。"""
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

for i in range(20):
    time.sleep(1)
    detail = evaluate("""(function(){
      var m = document.getElementById('messages');
      var out = [];
      for (var i = 0; i < m.children.length; i++) {
        out.push(m.children[i].className + ' :: ' + (m.children[i].innerText || '').slice(0, 100));
      }
      return JSON.stringify(out);
    })()""")
    items = json.loads(detail)
    last = items[-1] if items else ""
    print(f"[{i+1}s] last: {last}")
    if "assistant" in last and "reasoning" not in last:
        print("LATEST REPLY RENDERED OK")
        break
ws.close()
