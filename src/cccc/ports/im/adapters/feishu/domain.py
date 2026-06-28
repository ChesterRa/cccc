"""Feishu/Lark Open API domain normalization."""

from __future__ import annotations


FEISHU_DOMAIN = "https://open.feishu.cn"
LARK_DOMAIN = "https://open.larkoffice.com"


def normalize_domain(domain: str) -> str:
    value = str(domain or "").strip()
    if not value:
        return FEISHU_DOMAIN
    value = value.rstrip("/")
    if value.endswith("/open-apis"):
        value = value[: -len("/open-apis")].rstrip("/")
    if not (value.startswith("http://") or value.startswith("https://")):
        value = "https://" + value
    return value
