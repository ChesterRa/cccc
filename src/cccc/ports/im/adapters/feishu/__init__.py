"""Feishu adapter package."""

from .adapter import DEFAULT_MAX_CHARS, DEFAULT_MAX_LINES, FeishuAdapter
from .domain import FEISHU_DOMAIN, LARK_DOMAIN

__all__ = [
    "DEFAULT_MAX_CHARS",
    "DEFAULT_MAX_LINES",
    "FEISHU_DOMAIN",
    "LARK_DOMAIN",
    "FeishuAdapter",
]
