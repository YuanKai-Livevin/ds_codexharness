# -*- coding: utf-8 -*-
"""HARNESS 记忆面板后端入口（独立 sidecar）。

- 随机端口、Bearer 令牌鉴权（/api/* 受保护；静态面板页面开放，页面内请求自带令牌）。
- /responses 已移至独立网关（backend/gateway/main.py），本服务不再承担模型协议代理。
启动方式（由应用拉起或手动调试）：
  python -m uvicorn api.main:app --host 127.0.0.1 --port 8765
环境变量：
  HARNESS_DATA_DIR  记忆数据目录（默认 ./data）
  HARNESS_WORKSPACE 当前工作区路径（供交接文档引用）
  HARNESS_TOKEN     本地会话令牌（无令牌则拒绝一切 /api 请求）
  OH_API_KEY        DeepSeek 密钥（压缩/交接 LLM）
  TIKTOKEN_CACHE_DIR cl100k 编码缓存目录（离线计数）
"""
import os
from pathlib import Path

from fastapi import Depends, FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from .routes import router
from ..services.auth import require_token

app = FastAPI(
    title="HARNESS 记忆面板",
    description="自动压缩 + 可视化记忆块管理系统",
    version="0.3.0",
)

# CORS 收紧：仅允许本地 Tauri/面板来源（file:// 直开面板走同源静态，无跨域）
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "tauri://localhost",
        "https://tauri.localhost",
        "http://localhost",
        "http://127.0.0.1",
    ],
    allow_credentials=False,
    allow_methods=["GET", "POST", "PATCH", "DELETE"],
    allow_headers=["Content-Type", "Authorization"],
)

app.include_router(router, prefix="/api", dependencies=[Depends(require_token)])


@app.get("/api/health", dependencies=[Depends(require_token)])
def health():
    return {
        "ok": True,
        "service": "harness-memory",
        "workspace": os.environ.get("HARNESS_WORKSPACE", ""),
        "data_dir": os.environ.get("HARNESS_DATA_DIR", ""),
    }


# 托管前端面板（必须放在所有路由之后，否则根挂载会抢占 /api/*）：
# iframe（http://127.0.0.1:{随机端口}/）与浏览器直开均可用。
# main.py 位于 {block}/backend/api/，frontend 位于 {block}/frontend（三级上级）
_frontend_dir = os.environ.get("HARNESS_FRONTEND_DIR") or str(
    Path(__file__).resolve().parents[2] / "frontend"
)
if os.path.isdir(_frontend_dir):
    app.mount("/", StaticFiles(directory=_frontend_dir, html=True), name="frontend")
