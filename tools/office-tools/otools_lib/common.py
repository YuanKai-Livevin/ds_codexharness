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


# ================= T0-04 安全写入事务 =================
# 原则：默认不覆盖；覆盖必须先征得用户同意（overwrite=true）并自动备份；
#       先写临时文件 → 校验 → 原子替换；记录 old/new hash 供审计。

import hashlib
import shutil
import time as _time


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def backup_dir_for(root):
    """工作区内的覆盖备份目录（不占目标目录、不污染业务文件）。"""
    d = os.path.join(root, ".oh_tmp", "office-backups")
    os.makedirs(d, exist_ok=True)
    return d


def _tmp_for(out_path):
    """临时文件保留原扩展名（openpyxl/PIL/pypdf 按扩展名识别格式）。"""
    base, ext = os.path.splitext(out_path)
    return "%s.oh-tmp-%d-%d%s" % (base, os.getpid(), _time.time_ns() % 1000000, ext)


def prepare_output(out_path, overwrite):
    """目标存在且未批准覆盖 → CONFLICT；否则返回可安全写入的临时路径。"""
    if os.path.exists(out_path):
        if not overwrite:
            raise ToolError(
                "CONFLICT",
                f"输出文件已存在，默认不覆盖: {out_path}（如需覆盖，请先征得用户同意后传 overwrite: true）",
            )
    return _tmp_for(out_path)


def commit_output(tmp_path, out_path, overwrite, backup_dir=None):
    """原子提交：覆盖时先备份旧文件；返回 {backup, old_hash, new_hash}。"""
    info = {"backup": None, "old_hash": None, "new_hash": sha256_file(tmp_path)}
    if os.path.exists(out_path):
        info["old_hash"] = sha256_file(out_path)
        if not overwrite:
            os.remove(tmp_path)
            raise ToolError(
                "CONFLICT",
                f"输出文件已存在，默认不覆盖: {out_path}（如需覆盖，请先征得用户同意后传 overwrite: true）",
            )
        if backup_dir:
            os.makedirs(backup_dir, exist_ok=True)
            ts = _time.strftime("%Y%m%d-%H%M%S")
            bpath = os.path.join(backup_dir, "%s-%s" % (ts, os.path.basename(out_path)))
            shutil.copy2(out_path, bpath)
            info["backup"] = bpath
        os.replace(tmp_path, out_path)
    else:
        os.replace(tmp_path, out_path)
    return info


def check_conflicts(paths, overwrite):
    """批量输出预检：任一目标已存在且未批准覆盖 → 整体 CONFLICT（不写任何文件）。"""
    existing = [p for p in paths if os.path.exists(p)]
    if existing and not overwrite:
        names = ", ".join(os.path.basename(p) for p in existing[:5])
        more = f"（共 {len(existing)} 个）" if len(existing) > 5 else ""
        raise ToolError(
            "CONFLICT",
            f"输出文件已存在，默认不覆盖: {names}{more}（如需覆盖，请先征得用户同意后传 overwrite: true）",
        )


def safe_write(root, out_path, kind, overwrite, write_fn):
    """统一安全写入：临时文件 → 校验 → 原子替换（覆盖自动备份）。
    write_fn(tmp_path) 负责把内容写到临时路径。返回提交信息。"""
    out_path = resolve(root, out_path) if not os.path.isabs(out_path) else os.path.abspath(out_path)
    tmp = prepare_output(out_path, overwrite)
    try:
        write_fn(tmp)
        validate_output(tmp, kind)
        info = commit_output(tmp, out_path, overwrite, backup_dir_for(root))
        return info
    except Exception:
        if os.path.exists(tmp):
            try:
                os.remove(tmp)
            except OSError:
                pass
        raise
