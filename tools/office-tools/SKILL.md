---
name: office-tools
description: 办公高频任务的确定性工具运行时（Excel 合并/去重/筛选/透视/公式检查、Word 模板填充、PDF 合并/拆分/文本提取、图片批量缩放/转换、文件清单/批量重命名）。遇到这类任务时「选工具填参数」，不要手写脚本；先加 --dry-run 预演。
---

# office-tools：确定性办公工具

本技能提供 13 个稳定工具。执行办公高频任务时**优先调用这些工具**（选工具 + 填参数），而不是每次临时写 Python 脚本。工具本身带参数校验、权限边界（只能读写工作区）、dry-run 预演、输出校验与结构化错误码。

## 调用方式

```bash
python "<本技能目录>/otools.py" <工具名> --root "<工作区路径>" --params '{"参数": "值"}' [--dry-run]
```

- 本技能目录即 `skills/office-tools/`（`otools.py` 与其同级）。
- 所有输入/输出路径必须是工作区内的**相对路径**；越界会返回 `PERM_DENIED`。
- 不确定时先加 `--dry-run` 预演（只校验不写文件），确认后再正式执行。
- 输出为标准 JSON：`{ok, message, outputs[], warnings?, error_code?, 指标字段}`。
- 错误码：`SCHEMA_ERROR`(参数缺/错)、`FILE_NOT_FOUND`、`PERM_DENIED`(越界)、`EMPTY_INPUT`、`OUTPUT_INVALID`、`CONFLICT`(目标已存在)、`TOOL_FAILED`。
- 工具失败后先修参数重试；确实超出工具能力时，才允许回到写脚本的方式（放 .oh_tmp）。

## 工具清单与参数

### Excel
| 工具 | 参数 | 说明 |
|---|---|---|
| excel_merge | inputs[文件或目录], output, sheet?, header_rows? | 纵向合并多个 xlsx/csv（首文件表头） |
| excel_dedupe | input, output, key_columns?[列名或序号] | 按指定列去重（默认全列） |
| excel_filter | input, output, column, op(eq/ne/gt/ge/lt/le/contains), value | 按条件筛选行 |
| excel_pivot | input, output, rows[分组列], values(数值列), agg?(sum/count/avg) | 简单透视汇总 |
| excel_formula_check | input | 检查公式错误值（#DIV/0!、#REF! 等），无错误时 ok=true |

### Word
| 工具 | 参数 | 说明 |
|---|---|---|
| word_fill_template | input(模板), output, values{占位符:值} | 替换 `{{占位符}}`（段落+表格） |
| word_extract_text | input, output_txt? | 提取正文到 txt（缺省仅统计） |

### PDF
| 工具 | 参数 | 说明 |
|---|---|---|
| pdf_merge | inputs[], output | 合并多个 PDF |
| pdf_split | input, output_dir, ranges?[[起,止],...] / pages?[n,...] | 按页范围/单页拆分（1 基） |
| pdf_text | input, output_txt?, first_page?, last_page? | 提取文本 |

### 图片
| 工具 | 参数 | 说明 |
|---|---|---|
| img_resize | output_dir, width?/height?/percent?, inputs?(默认工作区图片) | 批量缩放（保持比例） |
| img_convert | output_dir, format(png/jpg/webp), inputs? | 批量格式转换 |

### 文件
| 工具 | 参数 | 说明 |
|---|---|---|
| file_manifest | input_dir?(默认工作区), output_csv?, include_hash?(默认true), pattern? | 生成文件清单（路径/大小/时间/SHA256） |
| file_rename | input_dir, mapping[{from,to},...] | 批量重命名（目标已存在则拒绝，不覆盖） |

## 示例

1. 合并三个报表：
```bash
python "C:\HARNESS\skills\office-tools\otools.py" excel_merge --root "F:\桌面文件\工作区" --params '{"inputs":["1月.xlsx","2月.xlsx","3月.xlsx"],"output":"合并.xlsx"}'
```

2. 先预演再执行去重：
```bash
python "C:\HARNESS\skills\office-tools\otools.py" excel_dedupe --root "F:\桌面文件\工作区" --params '{"input":"客户表.xlsx","output":"客户表-去重.xlsx","key_columns":["姓名","电话"]}' --dry-run
```

3. 合并 PDF 并生成文件清单：
```bash
python "C:\HARNESS\skills\office-tools\otools.py" pdf_merge --root "F:\桌面文件\工作区" --params '{"inputs":["a.pdf","b.pdf"],"output":"合并.pdf"}'
python "C:\HARNESS\skills\office-tools\otools.py" file_manifest --root "F:\桌面文件\工作区" --params '{"output_csv":"清单.csv"}'
```

## 边界
- 绝不修改本技能目录内的文件；它由 Harness 统一管理。
- 工具只接受工作区内路径；若用户需求超出工作区，按工作区铁律拒绝。
- **默认不覆盖**：输出文件已存在时返回 `CONFLICT`，绝不静默覆盖。若用户明确同意覆盖，才传 `"overwrite": true`，且工具会先把旧文件自动备份到工作区 `.oh_tmp/office-backups/` 再做原子替换（可恢复）；结果会报告旧/新文件 SHA-256 与备份路径。
- 覆盖前必须征得用户同意（先向用户确认，用户同意后再执行）；批量操作任一目标已存在且未批准时整体拒绝，不写任何文件。
- 批量重命名中途失败会自动反向回滚已完成的改名。
- 破坏性操作（重命名/覆盖输出）依旧遵循审批规则；`file_rename` 不覆盖已存在文件。
