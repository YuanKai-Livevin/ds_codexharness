# -*- coding: utf-8 -*-
"""本地 sidecar 令牌认证（T0-03）。

每次应用启动生成随机 HARNESS_TOKEN 注入环境；未带有效令牌的请求一律拒绝（fail closed）。
- /responses（模型网关）与 /api/*（记忆服务）都受保护；
- 静态面板页面（/ 与 /sidebar.html）无需令牌（页面加载），页面内请求自带令牌。
"""
import os

from fastapi import Header, HTTPException

_TOKEN = os.environ.get("HARNESS_TOKEN", "")


def require_token(authorization: str | None = Header(default=None)):
    """FastAPI 依赖：校验 Bearer 令牌。环境未配置令牌时拒绝一切请求（fail closed）。"""
    if not _TOKEN:
        raise HTTPException(status_code=503, detail="sidecar not configured (missing token)")
    if not authorization or not authorization.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="missing bearer token")
    if authorization[7:].strip() != _TOKEN:
        raise HTTPException(status_code=401, detail="invalid bearer token")
    return True
