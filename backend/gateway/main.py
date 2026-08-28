# -*- coding: utf-8 -*-
"""独立模型网关：仅暴露 /responses（Responses→Chat 翻译）与健康检查。

- 独立进程、随机端口、Bearer 令牌鉴权（随引擎生命周期启停）。
- 不再与记忆服务/静态面板共用进程与端口（T0-03）。
"""
import os

from fastapi import Depends, FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from ..services import bridge
from ..services.auth import require_token

app = FastAPI(title="HARNESS Model Gateway", version="0.3.0")

# 仅允许本地 Tauri/面板来源；其余跨域请求一律拒绝
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "tauri://localhost",
        "https://tauri.localhost",
        "http://localhost",
        "http://127.0.0.1",
    ],
    allow_credentials=False,
    allow_methods=["POST", "GET"],
    allow_headers=["Content-Type", "Authorization"],
)


@app.get("/api/health", dependencies=[Depends(require_token)])
def health():
    return {
        "ok": True,
        "service": "harness-gateway",
        "upstream": os.environ.get("HARNESS_UPSTREAM_URL", ""),
        "model": os.environ.get("HARNESS_UPSTREAM_MODEL", ""),
    }


@app.post("/responses", dependencies=[Depends(require_token)])
async def responses_proxy(request: Request):
    if not bridge.ENABLED:
        return JSONResponse({"error": "bridge disabled"}, status_code=404)
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid json"}, status_code=400)
    return bridge.handle(body)
