# -*- coding: utf-8 -*-
"""LLM 客户端：DeepSeek API（chat/completions 优先，responses 兜底）。

- 密钥来自环境变量 OH_API_KEY（应用启动时注入）
- 失败或无密钥时返回 None，由调用方降级为本地规则合并
"""
import json
import os

import requests

DEFAULT_BASE_URL = "https://api.deepseek.com/"
DEFAULT_MODEL = "deepseek-v4-flash"


def _api_key() -> str:
    return os.environ.get("OH_API_KEY", "").strip()


def available() -> bool:
    return bool(_api_key())


def _extract_responses_text(data: dict) -> str:
    """从 responses API 返回中鲁棒地提取文本（兜底路径）。"""
    parts = []
    out = data.get("output") or []
    for item in out:
        if not isinstance(item, dict):
            continue
        c = item.get("content")
        if isinstance(c, list):
            for seg in c:
                if isinstance(seg, dict) and seg.get("text"):
                    parts.append(seg["text"])
        elif isinstance(c, str) and c:
            parts.append(c)
        if isinstance(item.get("text"), str) and item["text"]:
            parts.append(item["text"])
    if not parts and data.get("output_text"):
        parts.append(data["output_text"])
    return "\n".join(parts).strip()


def chat(system: str, user: str, max_tokens: int = 3000, temperature: float = 0.3,
         timeout: float = 90.0, retries: int = 2) -> str | None:
    """调用 DeepSeek。chat/completions 优先；失败回退 responses API。失败仍返回 None。

    timeout：单次请求超时秒数；retries：重复尝试次数（缩短可让规则兜底更快生效）。
    支持内网免密钥部署：无 OH_API_KEY 时仍尝试调用（不带 Authorization 头）。
    """
    key = _api_key()
    base = os.environ.get("HARNESS_BASE_URL", DEFAULT_BASE_URL).rstrip("/")
    model = os.environ.get("HARNESS_MODEL", DEFAULT_MODEL)
    headers = {"Content-Type": "application/json"}
    if key:
        headers["Authorization"] = "Bearer " + key

    # 首选：chat/completions（系统角色可靠）
    for attempt in range(retries):
        try:
            body = {
                "model": model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "max_tokens": max_tokens,
                "temperature": temperature,
                "stream": False,
            }
            r = requests.post(base + "/chat/completions", json=body, headers=headers, timeout=timeout)
            if r.status_code == 200:
                data = r.json()
                content = (data.get("choices") or [{}])[0].get("message", {}).get("content")
                if isinstance(content, str) and content.strip():
                    return content.strip()
        except Exception:  # noqa: BLE001
            pass
        if attempt < retries - 1:
            import time
            time.sleep(1.0)

    # 兜底：responses API（与 codex 同款接线）
    try:
        body = {
            "model": model,
            "instructions": system,
            "input": [{"role": "user", "content": user}],
            "max_output_tokens": max_tokens,
            "temperature": temperature,
        }
        r = requests.post(base + "/responses", json=body, headers=headers, timeout=timeout)
        if r.status_code == 200:
            text = _extract_responses_text(r.json())
            if text:
                return text
    except Exception:  # noqa: BLE001
        pass
    return None


def chat_json(system: str, user: str, max_tokens: int = 3000, temperature: float = 0.3) -> list | dict | None:
    """请求 LLM 输出 JSON，并做鲁棒解析（容忍前言/围栏/多余文本）。"""
    text = chat(system, user, max_tokens=max_tokens, temperature=temperature)
    if not text:
        return None
    text = text.strip()
    # 剥离 ```json ... ``` 围栏
    if text.startswith("```"):
        text = re.sub(r"^```[a-zA-Z]*\s*", "", text)
        text = re.sub(r"\s*```$", "", text)
        text = text.strip()
    # 1) 整段直接可解析
    try:
        return json.loads(text)
    except Exception:  # noqa: BLE001
        pass
    # 2) 以最右闭合符为界，从左向右尝试每个开括号起点（容忍前言中的示例 JSON）
    for open_ch, close_ch in (("[", "]"), ("{", "}")):
        last = text.rfind(close_ch)
        if last == -1:
            continue
        idx = 0
        while True:
            i = text.find(open_ch, idx)
            if i == -1 or i > last:
                break
            try:
                return json.loads(text[i:last + 1])
            except Exception:  # noqa: BLE001
                idx = i + 1
    return None
