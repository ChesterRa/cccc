"""MCP 工具契约加载器。

完整工具名称、描述、annotations 与 inputSchema 由语言无关的
``resources/mcp_tools.json`` 唯一维护，Python 与 Rust 读取同一份契约。
"""

from __future__ import annotations

import json
from importlib.resources import files
from pathlib import Path
from typing import Any


def _load_contract() -> list[dict[str, Any]]:
    path = Path(__file__).resolve().parents[4] / "resources/mcp_tools.json"
    raw = (
        path.read_text(encoding="utf-8")
        if path.is_file()
        else files("cccc.resources").joinpath("mcp_tools.json").read_text(encoding="utf-8")
    )
    value = json.loads(raw)
    if not isinstance(value, list) or not all(isinstance(item, dict) for item in value):
        raise RuntimeError("resources/mcp_tools.json must contain a JSON tool array")
    return value


MCP_TOOLS = _load_contract()
