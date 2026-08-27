# -*- coding: utf-8 -*-
"""HARNESS 记忆面板后端入口。

启动方式（由应用拉起或手动调试）：
  python -m uvicorn api.main:app --host 127.0.0.1 --port 8765
环境变量：
  HARNESS_DATA_DIR  记忆数据目录（默认 ./data）
  HARNESS_WORKSPACE 当前工作区路径（供交接文档引用）
  OH_API_KEY        DeepSeek 密钥（压缩/交接 LLM）
  TIKTOKEN_CACHE_DIR cl100k 编码缓存目录（离线计数）
"""
import os
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from .routes import router
from ..services import bridge

app = FastAPI(
    title="HARNESS 记忆面板",
    description="自动压缩 + 可视化记忆块管理系统",
    version="0.3.0",
)

# 允许 file://（双击 HTML）与本地 WebView 访问
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=False,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(router, prefix="/api")


# 内置翻译层：Codex 的 /responses → 内网 /chat/completions（需在静态挂载前注册）
@app.post("/responses")
async def responses_proxy(request: Request):
    body = await request.json()
    return bridge.handle(body)


@app.get("/api/health")
def health():
    return {
        "ok": True,
        "service": "harness-memory",
        "workspace": os.environ.get("HARNESS_WORKSPACE", ""),
        "data_dir": os.environ.get("HARNESS_DATA_DIR", ""),
    }


# 托管前端面板（必须放在所有路由之后，否则根挂载会抢占 /api/*）：
# iframe（http://127.0.0.1:8765/）与浏览器直开均可用。
# main.py 位于 {block}/backend/api/，frontend 位于 {block}/frontend（三级上级）
_frontend_dir = os.environ.get("HARNESS_FRONTEND_DIR") or str(
    Path(__file__).resolve().parents[2] / "frontend"
)
if os.path.isdir(_frontend_dir):
    app.mount("/", StaticFiles(directory=_frontend_dir, html=True), name="frontend")
