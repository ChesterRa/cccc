"""Compatibility projection for the retired inclusive ``chat.read`` cursor."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class LegacyReadWatermark:
    """Resolve one actor generation's furthest valid inclusive read boundary."""

    event_positions: dict[str, int]
    inclusive_index: int | None

    @classmethod
    def from_events(
        cls, events: list[dict[str, Any]], *, actor_id: str
    ) -> "LegacyReadWatermark":
        event_positions: dict[str, int] = {}
        for index, event in enumerate(events):
            event_id = str(event.get("id") or "").strip()
            if event_id:
                event_positions.setdefault(event_id, index)

        inclusive_index: int | None = None
        for read_index, event in enumerate(events):
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            if (
                str(event.get("kind") or "") != "chat.read"
                or str(data.get("actor_id") or "").strip() != actor_id
            ):
                continue
            target_index = event_positions.get(str(data.get("event_id") or "").strip())
            if target_index is None or target_index > read_index:
                continue
            if inclusive_index is None or target_index > inclusive_index:
                inclusive_index = target_index

        return cls(
            event_positions=event_positions,
            inclusive_index=inclusive_index,
        )

    def covers_notification(self, event: dict[str, Any]) -> bool:
        if str(event.get("kind") or "") != "system.notify":
            return False
        if self.inclusive_index is None:
            return False
        event_index = self.event_positions.get(str(event.get("id") or "").strip())
        if event_index is not None and event_index <= self.inclusive_index:
            return True

        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        context = data.get("context") if isinstance(data.get("context"), dict) else {}
        referenced = (
            data.get("event_id"),
            data.get("related_event_id"),
            context.get("event_id"),
        )
        return any(
            (referenced_index := self.event_positions.get(str(event_id or "").strip()))
            is not None
            and referenced_index <= self.inclusive_index
            for event_id in referenced
        )
