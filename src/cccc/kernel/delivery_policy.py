from __future__ import annotations

from typing import Any

from ..util.conv import coerce_bool

DEFAULT_AUTO_MARK_ON_DELIVERY = True


def coerce_auto_mark_on_delivery(value: Any) -> bool:
    return coerce_bool(value, default=DEFAULT_AUTO_MARK_ON_DELIVERY)


def auto_mark_on_delivery_from_doc(delivery: Any) -> bool:
    if not isinstance(delivery, dict):
        return DEFAULT_AUTO_MARK_ON_DELIVERY
    return coerce_auto_mark_on_delivery(delivery.get("auto_mark_on_delivery"))
