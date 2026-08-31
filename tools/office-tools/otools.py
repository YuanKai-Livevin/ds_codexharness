# -*- coding: utf-8 -*-
"""office-tools：确定性办公工具运行时（R10 / T1-05）。

用法：
  otools.py <tool> --root <工作区> [--params '<json>'] [--dry-run]
  otools.py tools              # 列出全部工具与输入 schema
  otools.py selftest           # 运行内置单元测试

每个工具：
  - 输入 schema 校验（缺少/非法参数 → SCHEMA_ERROR）
  - 权限范围：所有路径必须落在 --root 工作区内（越界 → PERM_DENIED）
  - dry-run：只校验与预演，不写任何文件
  - 输出校验：生成后验证存在/非空/可重新打开（OUTPUT_INVALID）
  - 错误类型：SCHEMA_ERROR / FILE_NOT_FOUND / PERM_DENIED / TOOL_FAILED /
              OUTPUT_INVALID / EMPTY_INPUT / CONFLICT
  - 审计：由 Harness 引擎自动记录命令与结果；工具自身输出机器可读 JSON
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from otools_lib.common import ToolError, CODES, resolve, require, list_paths, validate_output, out  # noqa: E402
from otools_lib import excel_tools  # noqa: E402
from otools_lib.office_tools import (  # noqa: E402
    word_fill_template, word_extract_text,
    pdf_merge, pdf_split, pdf_text,
    img_resize, img_convert,
    file_manifest, file_rename,
)

TOOLS = {
    # Excel
    "excel_merge": (excel_tools.excel_merge, {
        "inputs": ["array"], "output": "string", "mode": "string(optional: vertical)",
        "sheet": "string(optional)", "header_rows": "int(optional, default 1)",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "excel_dedupe": (excel_tools.excel_dedupe, {
        "input": "string", "output": "string", "key_columns": "array(列名或序号, 可选, 默认全列)",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "excel_filter": (excel_tools.excel_filter, {
        "input": "string", "output": "string", "column": "string(列名或序号)",
        "op": "string: eq|ne|gt|ge|lt|le|contains", "value": "any",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "excel_pivot": (excel_tools.excel_pivot, {
        "input": "string", "output": "string", "rows": "array(分组列)",
        "values": "string(数值列)", "agg": "string: sum|count|avg(默认 sum)",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "excel_formula_check": (excel_tools.excel_formula_check, {
        "input": "string",
    }),
    # Word
    "word_fill_template": (word_fill_template, {
        "input": "string(docx 模板)", "output": "string", "values": "object({{占位符}}→值)",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "word_extract_text": (word_extract_text, {
        "input": "string(docx)", "output_txt": "string(可选，缺省仅输出统计)",
        "overwrite": "bool(optional)",
    }),
    # PDF
    "pdf_merge": (pdf_merge, {"inputs": ["array"], "output": "string",
                              "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)"}),
    "pdf_split": (pdf_split, {
        "input": "string", "output_dir": "string", "ranges": "array(如 [[1,3],[5,5]], 1 基含端点, 可选)",
        "pages": "array(如 [1,3,5], 可选)",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "pdf_text": (pdf_text, {
        "input": "string", "output_txt": "string(可选)", "first_page": "int(可选, 1 基)",
        "last_page": "int(可选)", "overwrite": "bool(optional)",
    }),
    # 图片
    "img_resize": (img_resize, {
        "inputs": ["array(文件或目录, 可选, 默认当前工作区图片)"], "output_dir": "string",
        "width": "int(可选)", "height": "int(可选)", "percent": "number(可选, 如 0.5)",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    "img_convert": (img_convert, {
        "inputs": ["array(可选, 默认当前工作区图片)"], "output_dir": "string",
        "format": "string: png|jpg|webp",
        "overwrite": "bool(optional, 默认 false；true 需先征得用户同意，覆盖自动备份)",
    }),
    # 文件
    "file_manifest": (file_manifest, {
        "input_dir": "string(可选, 默认工作区)", "output_csv": "string(可选)",
        "include_hash": "bool(可选, 默认 true)", "pattern": "string(可选, 文件名包含)",
        "overwrite": "bool(optional)",
    }),
    "file_rename": (file_rename, {
        "input_dir": "string", "mapping": "array([{from,to},...], 相对路径)", "dry_run": "bool(可选)",
    }),
}


def build_parser():
    p = argparse.ArgumentParser(prog="otools.py", description="确定性办公工具运行时")
    p.add_argument("tool", help="工具名，或 tools / selftest")
    p.add_argument("--root", default=os.getcwd(), help="工作区根目录（权限边界）")
    p.add_argument("--params", default="{}", help="参数 JSON")
    p.add_argument("--dry-run", action="store_true", help="只预演不写文件")
    return p


def main():
    # 统一 UTF-8 输出（Windows 控制台/管道编码不定，模型按 UTF-8 解析）
    if hasattr(sys.stdout, "reconfigure"):
        try:
            sys.stdout.reconfigure(encoding="utf-8")
        except Exception:  # noqa: BLE001
            pass
    args = build_parser().parse_args()
    tool = args.tool
    root = os.path.abspath(args.root)
    if not os.path.isdir(root):
        sys.stdout.write(json.dumps(out(False, f"工作区目录不存在: {root}", error_code="FILE_NOT_FOUND")))
        return 2
    if tool == "tools":
        sys.stdout.write(json.dumps({"ok": True, "tools": {k: v[1] for k, v in TOOLS.items()}}))
        return 0
    if tool == "selftest":
        from otools_lib.selftest import run_selftest
        return run_selftest(root)
    if tool not in TOOLS:
        sys.stdout.write(json.dumps(out(False, f"未知工具: {tool}（可用: {', '.join(TOOLS)}）", error_code="SCHEMA_ERROR")))
        return 2
    try:
        params = json.loads(args.params) if isinstance(args.params, str) else args.params
        if not isinstance(params, dict):
            raise ToolError("SCHEMA_ERROR", "params 必须是 JSON 对象")
        fn, _schema = TOOLS[tool]
        result = fn(params, root, dry_run=args.dry_run)
        sys.stdout.write(json.dumps(result, ensure_ascii=False))
        return 0 if result.get("ok") else 1
    except ToolError as e:
        sys.stdout.write(json.dumps(out(False, e.msg, error_code=e.code), ensure_ascii=False))
        return CODES.get(e.code, 1)
    except Exception as e:  # noqa: BLE001 —— 任何异常都转成结构化错误
        sys.stdout.write(json.dumps(out(False, f"工具异常: {e}", error_code="TOOL_FAILED"), ensure_ascii=False))
        return 4


if __name__ == "__main__":
    sys.exit(main())
