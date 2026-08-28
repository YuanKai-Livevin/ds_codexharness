# -*- coding: utf-8 -*-
"""公共：路径边界、参数校验、输出校验、错误码、统一输出格式。"""

import os


class ToolError(Exception):
    def __init__(self, code, msg):
        super().__init__(msg)
        self.code = code
        self.msg = msg


CODES = {
    "SCHEMA_ERROR": 1,
    "FILE_NOT_FOUND": 2,
    "PERM_DENIED": 3,
    "TOOL_FAILED": 4,
    "OUTPUT_INVALID": 5,
    "EMPTY_INPUT": 6,
    "CONFLICT": 7,
}


def out(ok, message, outputs=None, warnings=None, error_code=None, extra=None):
    """统一输出结构（机器可读，模型直接引用）。"""
    d = {"ok": bool(ok), "message": message}
    if outputs:
        d["outputs"] = list(outputs)
    if warnings:
        d["warnings"] = list(warnings)
    if error_code:
        d["error_code"] = error_code
    if extra:
        d.update(extra)
    return d


def resolve(root, p):
    """路径解析：相对 root 或绝对路径均可，但必须落在 root 内（权限边界）。"""
    if not p or not str(p).strip():
        raise ToolError("SCHEMA_ERROR", "缺少路径参数")
    s = str(p).strip()
    a = os.path.abspath(s) if os.path.isabs(s) else os.path.abspath(os.path.join(root, s))
    r = os.path.abspath(root)
    try:
        common = os.path.commonpath([r, a])
    except ValueError:
        common = ""
    if common != r:
        raise ToolError("PERM_DENIED", f"路径越界（仅允许工作区内）: {p}")
    return a


def require(keys, params, optional=()):
    miss = [k for k in keys if params.get(k) in (None, "")]
    if miss:
        raise ToolError("SCHEMA_ERROR", f"缺少必需参数: {', '.join(miss)}")
    return {k: params.get(k) for k in list(keys) + list(optional)}


def list_paths(root, entries, exts=None):
    """把「文件路径或目录」列表展开为文件列表；目录递归收集；按扩展名过滤。"""
    out_files = []
    for e in entries or []:
        p = resolve(root, e)
        if os.path.isdir(p):
            for base, _dirs, files in os.walk(p):
                for f in sorted(files):
                    fp = os.path.join(base, f)
                    if exts is None or os.path.splitext(fp)[1].lower() in exts:
                        out_files.append(fp)
        elif os.path.isfile(p):
            if exts is None or os.path.splitext(p)[1].lower() in exts:
                out_files.append(p)
        else:
            raise ToolError("FILE_NOT_FOUND", f"路径不存在: {e}")
    return out_files


def validate_output(path, kind):
    """输出校验：存在 + 非空 + 可重新打开（按类型）。"""
    if not os.path.exists(path):
        raise ToolError("OUTPUT_INVALID", f"输出文件未生成: {path}")
    if os.path.getsize(path) == 0:
        raise ToolError("OUTPUT_INVALID", f"输出文件为空: {path}")
    try:
        if kind == "xlsx":
            import openpyxl
            wb = openpyxl.load_workbook(path, read_only=True)
            wb.close()
        elif kind == "docx":
            import docx
            docx.Document(path)
        elif kind == "pdf":
            from pypdf import PdfReader
            PdfReader(path)
        elif kind == "image":
            from PIL import Image
            img = Image.open(path)
            img.verify()
        elif kind == "csv":
            with open(path, "r", encoding="utf-8-sig") as fh:
                if not fh.read(1):
                    raise ToolError("OUTPUT_INVALID", f"CSV 为空: {path}")
        elif kind == "any":
            pass
    except ToolError:
        raise
    except Exception as e:  # noqa: BLE001
        raise ToolError("OUTPUT_INVALID", f"输出文件无法打开校验: {path}（{e}）")
    return True


def ensure_dir(path):
    os.makedirs(path, exist_ok=True)


def cell_value(v):
    """Excel 单元格值 → 可 JSON 化。"""
    if v is None:
        return ""
    if isinstance(v, (str, int, float, bool)):
        return v
    return str(v)
