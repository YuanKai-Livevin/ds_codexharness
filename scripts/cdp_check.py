#!/usr/bin/env python3
"""通过 CDP 检查 v0.3 前端状态（无 Origin 头连接）。"""
import json
import urllib.request
import websocket

targets = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json/list", timeout=5))
page = next((t for t in targets if t.get("type") == "page"), targets[0])
ws = websocket.create_connection(page["webSocketDebuggerUrl"], timeout=10, origin=None)
mid = 0

def evaluate(expr):
    global mid
    mid += 1
    ws.send(json.dumps({"id": mid, "method": "Runtime.evaluate", "params": {"expression": expr, "returnByValue": True}}))
    while True:
        r = json.loads(ws.recv())
        if r.get("id") == mid:
            return r.get("result", {}).get("result", {}).get("value")

print("target:", page.get("title"))
print("__TAURI__:", evaluate("typeof window.__TAURI__"))
print("__TAURI__.event:", evaluate("typeof (window.__TAURI__ && window.__TAURI__.event)"))
print("__TAURI__.core:", evaluate("typeof (window.__TAURI__ && window.__TAURI__.core)"))
print("fatal-bar:", evaluate("var b=document.getElementById('fatal-bar'); b?b.textContent:null"))
print("chip:", evaluate("document.getElementById('engine-chip').textContent"))
print("messages children:", evaluate("document.getElementById('messages').children.length"))
print("send disabled:", evaluate("document.getElementById('btn-send').disabled"))
print("inject test:", evaluate("(function(){handleEvent({type:'agentDelta',text:'TEST-REPLY-OK'});return document.getElementById('messages').children.length})()"))
print("last msg:", evaluate("var m=document.getElementById('messages'); m.lastElementChild?m.lastElementChild.innerText:null"))
ws.close()
