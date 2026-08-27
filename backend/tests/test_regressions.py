# -*- coding: utf-8 -*-
"""T0 关键缺陷回归测试。

- T0-05：记忆压缩候选集——只有 eligible（10 轮前的 ACTIVE 非置顶）块会被压缩，
  新块 / 暂停 / 置顶 / 已归档块绝不能被删除或改状态。
- T0-01：bridge 工具解析——支持官方 Codex 扁平 Responses 工具结构与旧嵌套结构。
"""
import os
import sys
import tempfile
from datetime import datetime, timedelta

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")))

from backend.models.memory import BlockStatus, BlockType, MemoryBlock
from backend.services import compactor, watermark
from backend.services.storage import MemoryStore

# 确保走规则合并路径（无 LLM Key）
os.environ.pop("OH_API_KEY", None)


def _mk(id_, typ, content, status=BlockStatus.ACTIVE, importance=3,
        pinned=False, source_round=0, last_access_dt=None):
    return MemoryBlock(
        id=id_, type=typ, content=content, importance=importance, status=status,
        token_count=watermark.count_tokens(content),
        last_accessed=last_access_dt or datetime.now(),
        source_round=source_round, is_pinned=pinned, order_index=0,
    )


def test_t005_compactor_candidate_set():
    store = MemoryStore(tempfile.mkdtemp(prefix="hm-test-"))
    now = datetime.now()
    old = now - timedelta(hours=3)
    long_txt = "销售数据文件位于工作区根目录，文件名销售数据-2025.xlsx 共 12 张表" * 3

    # 2 个 eligible（旧、ACTIVE、非置顶、内容重复→规则合并可显著减量）
    pool = [
        _mk("old1", BlockType.FACT, long_txt, source_round=0, last_access_dt=old),
        _mk("old2", BlockType.FACT, long_txt, source_round=1, last_access_dt=old),
        # 不应被触碰的块
        _mk("new1", BlockType.TASK, "新任务：生成月度报表", source_round=99),          # 新块
        _mk("paused", BlockType.FACT, "暂停的旧块", status=BlockStatus.PAUSED, source_round=0, last_access_dt=old),
        _mk("pinned", BlockType.PLAN, "置顶计划块", pinned=True, source_round=0, last_access_dt=old),
        _mk("dep", BlockType.FACT, "已归档块", status=BlockStatus.DEPRECATED, source_round=0, last_access_dt=old),
    ]
    store.save_blocks(pool)
    store.update_conversation(52000, 20)   # current_round=20 → old1/old2 距今 20/19 轮

    report = compactor.compress(store)
    assert report["ok"], report
    assert report["compacted"] == 2, report          # 只压缩 2 个 eligible
    assert not report["ineffective"], report

    after = {b.id: b for b in store.load_blocks()}
    # eligible 被合并为 1 个 probation 块
    merged = [b for b in after.values() if b.status == BlockStatus.PROBATION]
    assert len(merged) == 1, [b.id for b in after.values()]
    # 其它块必须原样保留
    assert after["new1"].status == BlockStatus.ACTIVE, "新块被误动！"
    assert after["paused"].status == BlockStatus.PAUSED, "暂停块被误动！"
    assert after["pinned"].status == BlockStatus.ACTIVE and after["pinned"].is_pinned, "置顶块被误动！"
    assert after["dep"].status == BlockStatus.DEPRECATED, "已归档块被误动！"
    print("T0-05 PASS: 只有 eligible 块被压缩，其余状态不变")


def test_t001_bridge_flat_and_nested_tools():
    from backend.services import bridge

    # 官方 Codex 扁平 Responses 工具结构
    flat = {
        "tools": [{
            "type": "function",
            "name": "run_bash",
            "description": "运行 shell 命令",
            "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}},
            "strict": False,
        }]
    }
    tools = bridge._tools_from_body(flat)
    assert tools and tools[0]["type"] == "function"
    assert tools[0]["function"]["name"] == "run_bash", tools
    assert tools[0]["function"]["parameters"]["properties"]["cmd"], tools

    # 旧嵌套结构
    nested = {"tools": [{"type": "function", "function": {
        "name": "edit_file", "description": "编辑文件",
        "parameters": {"type": "object", "properties": {"path": {"type": "string"}}},
    }}]}
    tools2 = bridge._tools_from_body(nested)
    assert tools2 and tools2[0]["function"]["name"] == "edit_file", tools2

    # 未知工具类型不静默丢弃（告警）+ 混合场景不崩溃
    mixed = {"tools": [flat["tools"][0], {"type": "custom", "name": "x"}]}
    tools3 = bridge._tools_from_body(mixed)
    assert tools3 and len(tools3) == 1 and tools3[0]["function"]["name"] == "run_bash", tools3
    print("T0-01 PASS: 扁平/嵌套工具均可转换，未知类型仅告警")


if __name__ == "__main__":
    test_t005_compactor_candidate_set()
    test_t001_bridge_flat_and_nested_tools()
    print("全部回归测试通过")
