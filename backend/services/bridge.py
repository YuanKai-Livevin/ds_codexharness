# -*- coding: utf-8 -*-
"""OpenAI Responses API → Chat Completions 翻译层（内网网关兼容）。

Codex 引擎只支持 OpenAI Responses API（/responses）；当内网 DeepSeek
网关仅支持 /chat/completions 时，本层把 codex 的 /responses 请求翻译成
/chat/completions 转发到真实上游，再把结果（含 SSE 流式与函数调用）转回。

开关：环境变量 HARNESS_BRIDGE=1 启用；上游地址 HARNESS_UPSTREAM_URL，
模型 HARNESS_UPSTREAM_MODEL，密钥 OH_API_KEY（内网免密钥可留空）。
"""
import json
import os
import time
import uuid

import requests
from fastapi.responses import JSONResponse, StreamingResponse

UPSTREAM = os.environ.get("HARNESS_UPSTREAM_URL", "").rstrip("/")
MODEL = os.environ.get("HARNESS_UPSTREAM_MODEL", "")
KEY = os.environ.get("OH_API_KEY", "").strip()
ENABLED = os.environ.get("HARNESS_BRIDGE", "0") == "1"

_CHAT_URL = (UPSTREAM.rstrip("/") + "/chat/completions") if UPSTREAM else ""
_MODELS_URL = (UPSTREAM.rstrip("/") + "/models") if UPSTREAM else ""


def _headers() -> dict:
    h = {"Content-Type": "application/json"}
    if KEY and KEY != "EMPTY":
        h["Authorization"] = "Bearer " + KEY
    return h


# ---------- 请求翻译：Responses → Chat ----------

def _content_to_text(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for seg in content:
            if isinstance(seg, dict):
                t = seg.get("text")
                if t:
                    parts.append(str(t))
                elif seg.get("type") in ("input_image", "input_image_url"):
                    parts.append("[图片]")
        return "\n".join(parts)
    return str(content or "")


def _input_to_messages(input_items, instructions: str) -> list:
    messages = []
    if instructions:
        messages.append({"role": "system", "content": instructions})
    for it in input_items or []:
        if not isinstance(it, dict):
            continue
        typ = it.get("type")
        # 防御：无 type 但带 role 的项按 message 处理
        if typ is None and it.get("role"):
            typ = "message"
        if typ == "message":
            role = it.get("role", "user")
            text = _content_to_text(it.get("content"))
            if role in ("developer", "system"):
                messages.append({"role": "system", "content": text})
            else:
                messages.append({"role": role, "content": text})
        elif typ == "function_call":
            args = it.get("arguments") or "{}"
            if isinstance(args, (dict, list)):
                args = json.dumps(args, ensure_ascii=False)
            cid = it.get("call_id") or it.get("id") or ""
            messages.append({
                "role": "assistant",
                "content": None,
                "tool_calls": [{
                    "id": cid,
                    "type": "function",
                    "function": {"name": it.get("name", ""), "arguments": str(args)},
                }],
            })
        elif typ == "function_call_output":
            messages.append({
                "role": "tool",
                "tool_call_id": it.get("call_id") or "",
                "content": str(it.get("output") or ""),
            })
        # reasoning 等其他类型直接忽略
    return messages


def _tools_from_body(body) -> list | None:
    tools = []
    for t in body.get("tools") or []:
        fn = t.get("function") if isinstance(t, dict) else None
        if fn:
            tools.append({
                "type": "function",
                "function": {
                    "name": fn.get("name", ""),
                    "description": fn.get("description", ""),
                    "parameters": fn.get("parameters", {"type": "object", "properties": {}}),
                },
            })
    return tools or None


def _build_chat_body(body: dict) -> dict:
    chat = {
        "model": MODEL,
        "messages": _input_to_messages(body.get("input"), body.get("instructions") or ""),
        "stream": bool(body.get("stream", False)),
        "temperature": body.get("temperature", 0.3),
    }
    if "max_output_tokens" in body and body["max_output_tokens"]:
        chat["max_tokens"] = body["max_output_tokens"]
    tools = _tools_from_body(body)
    if tools:
        chat["tools"] = tools
        if "tool_choice" in body and body["tool_choice"]:
            chat["tool_choice"] = body["tool_choice"]
    return chat


# ---------- 响应翻译：Chat → Responses（非流式） ----------

def _resp_id() -> str:
    return "resp_" + uuid.uuid4().hex[:20]


def _chat_to_responses(chat_resp: dict, rid: str) -> dict:
    choice = (chat_resp.get("choices") or [{}])[0]
    msg = choice.get("message") or {}
    usage = chat_resp.get("usage") or {}
    items = []
    if msg.get("content"):
        items.append({
            "type": "message",
            "id": "msg_" + uuid.uuid4().hex[:16],
            "role": "assistant",
            "content": [{"type": "output_text", "text": msg["content"]}],
            "status": "completed",
        })
    for tc in msg.get("tool_calls") or []:
        cid = tc.get("id", "call_" + uuid.uuid4().hex[:12])
        items.append({
            "type": "function_call",
            "id": cid,
            "call_id": cid,
            "name": (tc.get("function") or {}).get("name", ""),
            "arguments": (tc.get("function") or {}).get("arguments", "{}"),
            "status": "completed",
        })
    return {
        "id": rid,
        "object": "response",
        "created_at": int(time.time()),
        "status": "completed",
        "model": MODEL,
        "output": items,
        "usage": {
            "input_tokens": usage.get("prompt_tokens", 0),
            "output_tokens": usage.get("completion_tokens", 0),
            "total_tokens": usage.get("total_tokens", 0),
        },
    }


# ---------- 响应翻译：Chat → Responses（SSE 流式） ----------

def _sse(payload: dict) -> str:
    return "data: " + json.dumps(payload, ensure_ascii=False) + "\n\n"


def _translate_stream(body: dict, rid: str):
    chat = _build_chat_body(body)
    upstream = requests.post(
        _CHAT_URL, json=chat, headers=_headers(), stream=True, timeout=300
    )
    if upstream.status_code != 200:
        err = upstream.text[:300] if hasattr(upstream, "text") else ""
        return _error_response("upstream HTTP {}: {}".format(upstream.status_code, err), rid)

    msg_item_id = "msg_" + uuid.uuid4().hex[:16]
    reasoning_item_id = "rsn_" + uuid.uuid4().hex[:16]
    call_items = {}   # index -> {id, name, args, item_id}
    collected = {
        "message": {"id": msg_item_id, "role": "assistant", "content": [{"type": "output_text", "text": ""}], "status": "in_progress"},
        "tool_calls": [],
        "reasoning": {"id": reasoning_item_id, "type": "reasoning", "summary": [{"type": "summary_text", "text": ""}], "status": "in_progress"},
    }

    def gen():
        yield _sse({"type": "response.created", "response": {"id": rid, "object": "response", "status": "in_progress", "model": MODEL}})
        yield _sse({"type": "response.in_progress", "response": {"id": rid, "object": "response", "status": "in_progress", "model": MODEL}})
        tool_added = False
        reasoning_started = False
        text_started = False
        for raw in upstream.iter_lines(decode_unicode=True):
            if not raw:
                continue
            line = raw.strip()
            if not line.startswith("data:"):
                continue
            data = line[5:].strip()
            if data == "[DONE]":
                break
            try:
                obj = json.loads(data)
            except Exception:
                continue
            choices = obj.get("choices") or []
            if not choices:
                continue
            delta = choices[0].get("delta") or {}

            # 推理内容（reasoning_content）→ responses reasoning 事件
            rsn = delta.get("reasoning_content")
            if rsn:
                if not reasoning_started:
                    yield _sse({"type": "response.output_item.added", "output_index": 0, "item": collected["reasoning"]})
                    reasoning_started = True
                collected["reasoning"]["summary"][0]["text"] += rsn
                yield _sse({"type": "response.reasoning_summary_text.delta", "item_id": reasoning_item_id, "output_index": 0, "delta": rsn})

            # 文本增量
            content = delta.get("content")
            if content:
                if not text_started:
                    yield _sse({"type": "response.output_item.added", "output_index": 0, "item": collected["message"]})
                    yield _sse({"type": "response.content_part.added", "item_id": msg_item_id, "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": ""}})
                    text_started = True
                collected["message"]["content"][0]["text"] += content
                yield _sse({"type": "response.output_text.delta", "item_id": msg_item_id, "output_index": 0, "content_index": 0, "delta": content})

            # 工具调用增量
            for tc in delta.get("tool_calls") or []:
                idx = tc.get("index", 0)
                fn = tc.get("function") or {}
                if idx not in call_items:
                    call_items[idx] = {
                        "id": tc.get("id", "call_" + uuid.uuid4().hex[:12]),
                        "name": fn.get("name", ""),
                        "args": "",
                        "item_id": "fc_" + uuid.uuid4().hex[:16],
                    }
                    if not tool_added:
                        yield _sse({"type": "response.output_item.added", "output_index": 0, "item": {
                            "type": "function_call", "id": call_items[idx]["item_id"], "call_id": call_items[idx]["id"],
                            "name": "", "arguments": "", "status": "in_progress",
                        }})
                        tool_added = True
                    if fn.get("name"):
                        call_items[idx]["name"] += fn["name"]
                        yield _sse({"type": "response.function_call_arguments.delta", "item_id": call_items[idx]["item_id"], "output_index": 0, "delta": fn["name"]})
                if fn.get("arguments"):
                    call_items[idx]["args"] += fn["arguments"]
                    yield _sse({"type": "response.function_call_arguments.delta", "item_id": call_items[idx]["item_id"], "output_index": 0, "delta": fn["arguments"]})

        # 收尾事件
        if reasoning_started:
            collected["reasoning"]["status"] = "completed"
            yield _sse({"type": "response.reasoning_summary_text.done", "item_id": reasoning_item_id, "output_index": 0, "text": collected["reasoning"]["summary"][0]["text"]})
            yield _sse({"type": "response.output_item.done", "output_index": 0, "item": collected["reasoning"]})
        if text_started:
            yield _sse({"type": "response.output_text.done", "item_id": msg_item_id, "output_index": 0, "content_index": 0, "text": collected["message"]["content"][0]["text"]})
            yield _sse({"type": "response.content_part.done", "item_id": msg_item_id, "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": collected["message"]["content"][0]["text"]}})
            collected["message"]["status"] = "completed"
            yield _sse({"type": "response.output_item.done", "output_index": 0, "item": collected["message"]})
        output_items = []
        if reasoning_started:
            output_items.append(collected["reasoning"])
        if text_started:
            output_items.append(collected["message"])
        for idx in sorted(call_items.keys()):
            ci = call_items[idx]
            item = {
                "type": "function_call",
                "id": ci["item_id"],
                "call_id": ci["id"],
                "name": ci["name"],
                "arguments": ci["args"],
                "status": "completed",
            }
            yield _sse({"type": "response.function_call_arguments.done", "item_id": ci["item_id"], "output_index": 0, "arguments": ci["args"]})
            yield _sse({"type": "response.output_item.done", "output_index": 0, "item": item})
            output_items.append(item)
        yield _sse({"type": "response.completed", "response": {
            "id": rid, "object": "response", "status": "completed", "model": MODEL, "output": output_items,
            "created_at": int(time.time()),
        }})
        yield "data: [DONE]\n\n"

    return StreamingResponse(gen(), media_type="text/event-stream")


def _error_response(message: str, rid: str):
    payload = {
        "id": rid, "object": "response", "status": "failed", "model": MODEL,
        "error": {"code": "bridge_upstream_error", "message": message},
    }
    return JSONResponse(payload, status_code=502)


def handle(body: dict):
    """入口：处理 codex 发来的 /responses 请求。"""
    rid = body.get("id") or _resp_id()
    if not ENABLED:
        return JSONResponse({"error": "bridge disabled"}, status_code=404)
    if not _CHAT_URL:
        return _error_response("未配置上游地址（HARNESS_UPSTREAM_URL）", rid)
    if not MODEL:
        return _error_response("未配置上游模型（HARNESS_UPSTREAM_MODEL）", rid)
    if body.get("stream"):
        return _translate_stream(body, rid)
    try:
        chat = _build_chat_body(body)
        upstream = requests.post(_CHAT_URL, json=chat, headers=_headers(), timeout=300)
        if upstream.status_code != 200:
            return _error_response("upstream HTTP {}: {}".format(upstream.status_code, upstream.text[:300]), rid)
        return _chat_to_responses(upstream.json(), rid)
    except Exception as e:  # noqa: BLE001
        return _error_response(str(e), rid)
