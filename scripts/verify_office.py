#!/usr/bin/env python3
"""验证内置 Python 办公能力：合并两个销售 Excel 并按区域汇总销售额。"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "runtime", "python312"))
import pandas as pd

WS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "demo-workspace")

frames = []
for f in ["销售数据-2025.xlsx", "销售数据-2026.xlsx"]:
    df = pd.read_excel(os.path.join(WS, f))
    frames.append(df)

merged = pd.concat(frames, ignore_index=True)
summary = merged.groupby("区域").agg(总销量=("销量", "sum"), 总销售额=("销售额", "sum")).reset_index()

out = os.path.join(WS, "销售汇总.xlsx")
with pd.ExcelWriter(out, engine="openpyxl") as writer:
    merged.to_excel(writer, sheet_name="合并明细", index=False)
    summary.to_excel(writer, sheet_name="区域汇总", index=False)

print(f"合并完成: {len(merged)} 行 × {merged.shape[1]} 列")
print("按区域汇总:")
print(summary.to_string(index=False))
print(f"产出物: {out}")
