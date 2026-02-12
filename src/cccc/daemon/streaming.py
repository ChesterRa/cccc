from __future__ import annotations

import json
import logging
import queue
import socket
import threading
import time
from dataclasses import dataclass
from typing import Any, Dict, Iterable, Optional, Set

from ..kernel.group import load_group
from ..kernel.inbox import is_message_for_actor
from ..util.time import parse_utc_iso, utc_now_iso

logger = logging.getLogger(__name__)


STREAMABLE_KINDS_V1: Set[str] = {
    "chat.message",
    "chat.ack",
    "system.notify",
    "system.notify_ack",
}


def _send_ndjson(sock: socket.socket, obj: Dict[str, Any]) -> None:
    data = (json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8")
    sock.sendall(data)


@dataclass(frozen=True)
class EventStreamSubscription:
    sub_id: str
    group_id: str
    by: str
    kinds: Optional[Set[str]]
    q: "queue.Queue[Optional[Dict[str, Any]]]"
    is_actor_view: bool


class EventBroadcaster:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._subs: Dict[str, EventStreamSubscription] = {}
        self._group_cache: Dict[str, tuple[Any, float]] = {}
        self._seq = 0

    @staticmethod
    def _signal_close(q: "queue.Queue[Optional[Dict[str, Any]]]") -> None:
        try:
            q.put_nowait(None)
            return
        except queue.Full:
            pass
        except Exception:
            return

        # Queue is full: drop buffered items so the consumer can observe the close signal.
        try:
            while True:
                q.get_nowait()
        except queue.Empty:
            pass
        except Exception:
            return

        try:
            q.put_nowait(None)
        except Exception:
            return

    def subscribe(
        self,
        *,
        group_id: str,
        by: str,
        kinds: Optional[Set[str]],
        max_queue: int = 2048,
    ) -> EventStreamSubscription:
        gid = str(group_id or "").strip()
        who = str(by or "").strip() or "user"
        allow = set(kinds) if kinds else None
        if allow is not None:
            allow = {k for k in allow if k in STREAMABLE_KINDS_V1}
            if not allow:
                allow = None

        is_actor_view = False
        g = load_group(gid) if gid else None
        if g is not None and who and who != "user":
            actors = g.doc.get("actors")
            if isinstance(actors, list):
                is_actor_view = any(
                    isinstance(a, dict) and str(a.get("id") or "").strip() == who for a in actors
                )

        with self._lock:
            self._seq += 1
            sub_id = f"s{self._seq:x}"
            sub = EventStreamSubscription(
                sub_id=sub_id,
                group_id=gid,
                by=who,
                kinds=allow,
                q=queue.Queue(maxsize=max(1, int(max_queue))),
                is_actor_view=bool(is_actor_view),
            )
            self._subs[sub_id] = sub
            return sub

    def unsubscribe(self, sub: EventStreamSubscription) -> None:
        sid = sub.sub_id
        with self._lock:
            self._subs.pop(sid, None)
        self._signal_close(sub.q)

    def close(self, sub: EventStreamSubscription) -> None:
        sid = sub.sub_id
        with self._lock:
            self._subs.pop(sid, None)
        self._signal_close(sub.q)

    def on_append(self, event: Dict[str, Any]) -> None:
        self.publish(event)

    def _load_group_cached(self, group_id: str) -> Optional[Any]:
        gid = str(group_id or "").strip()
        if not gid:
            return None
        now = time.monotonic()
        with self._lock:
            cached = self._group_cache.get(gid)
            if cached is not None:
                g, loaded_at = cached
                if (now - float(loaded_at)) < 2.0:
                    return g

        g = load_group(gid)

        with self._lock:
            if g is None:
                self._group_cache.pop(gid, None)
                return None
            self._group_cache[gid] = (g, now)
            return g

    def _iter_targets(self, event: Dict[str, Any]) -> Iterable[EventStreamSubscription]:
        gid = str(event.get("group_id") or "").strip()
        if not gid:
            return []
        with self._lock:
            return [s for s in self._subs.values() if s.group_id == gid]

    def publish(self, event: Dict[str, Any]) -> None:
        kind = str(event.get("kind") or "").strip()
        if kind not in STREAMABLE_KINDS_V1:
            return

        targets = list(self._iter_targets(event))
        if not targets:
            return

        group: Optional[Any] = None
        for sub in targets:
            if sub.kinds is not None and kind not in sub.kinds:
                continue

            if sub.is_actor_view:
                if kind not in ("chat.message", "system.notify"):
                    continue
                if group is None:
                    group = self._load_group_cached(sub.group_id)
                if group is None:
                    continue
                if kind == "chat.message" and str(event.get("by") or "").strip() == sub.by:
                    continue
                if not is_message_for_actor(group, actor_id=sub.by, event=event):
                    continue

            try:
                sub.q.put_nowait(event)
            except queue.Full:
                # Backpressure: drop oldest events instead of closing the connection
                dropped = 0
                try:
                    # Drop up to 10% of queue capacity to make room
                    drop_count = max(1, sub.q.maxsize // 10)
                    for _ in range(drop_count):
                        try:
                            sub.q.get_nowait()
                            dropped += 1
                        except queue.Empty:
                            break
                    sub.q.put_nowait(event)
                except queue.Full:
                    # Still full after dropping, give up on this event
                    dropped += 1
                if dropped > 0:
                    logger.warning(
                        "Event stream backpressure: dropped %d event(s) for sub %s (group=%s, by=%s)",
                        dropped, sub.sub_id, sub.group_id, sub.by
                    )


EVENT_BROADCASTER = EventBroadcaster()


def _parse_kinds_arg(value: Any) -> Optional[Set[str]]:
    if value is None:
        return None
    if isinstance(value, list):
        items = {str(x).strip() for x in value if isinstance(x, str) and str(x).strip()}
        items = {k for k in items if k in STREAMABLE_KINDS_V1}
        return items or None
    return None


def _tail_events(group_id: str, *, max_lines: int = 2000) -> list[Dict[str, Any]]:
    g = load_group(group_id)
    if g is None:
        return []
    try:
        from ..kernel.ledger import read_last_lines

        lines = read_last_lines(g.ledger_path, int(max_lines))
    except Exception:
        return []
    out: list[Dict[str, Any]] = []
    for ln in lines:
        try:
            obj = json.loads(ln)
        except Exception:
            continue
        if isinstance(obj, dict):
            out.append(obj)
    return out


def _resume_candidates(
    group_id: str,
    *,
    since_event_id: str,
    since_ts: str,
    kinds: Optional[Set[str]],
) -> list[Dict[str, Any]]:
    tail = _tail_events(group_id)
    if not tail:
        return []

    filtered = []
    for ev in tail:
        k = str(ev.get("kind") or "").strip()
        if k not in STREAMABLE_KINDS_V1:
            continue
        if kinds is not None and k not in kinds:
            continue
        filtered.append(ev)

    if since_event_id:
        for i, ev in enumerate(filtered):
            if str(ev.get("id") or "").strip() == since_event_id:
                return filtered[i + 1 :]
        # If the event id is no longer in tail history, fall back to timestamp-based
        # best-effort resume when available.
        if not since_ts:
            return []

    if since_ts:
        cutoff = parse_utc_iso(since_ts)
        if cutoff is None:
            return []
        out: list[Dict[str, Any]] = []
        for ev in filtered:
            ts = parse_utc_iso(str(ev.get("ts") or ""))
            if ts is not None and ts > cutoff:
                out.append(ev)
        return out

    return []


def stream_events_to_socket(
    *,
    sock: socket.socket,
    group_id: str,
    by: str,
    kinds: Optional[Set[str]] = None,
    since_event_id: str = "",
    since_ts: str = "",
    heartbeat_seconds: int = 30,
) -> None:
    sub = EVENT_BROADCASTER.subscribe(group_id=group_id, by=by, kinds=kinds)
    recent: Dict[str, None] = {}

    # Avoid indefinite blocking if a client stops reading.
    try:
        sock.settimeout(5.0)
    except Exception:
        pass

    def _seen(event_id: str) -> bool:
        eid = str(event_id or "").strip()
        if not eid:
            return False
        if eid in recent:
            return True
        recent[eid] = None
        if len(recent) > 2048:
            try:
                recent.pop(next(iter(recent)), None)
            except Exception:
                recent.clear()
        return False

    try:
        try:
            if heartbeat_seconds <= 0:
                heartbeat_seconds = 30
            if heartbeat_seconds > 300:
                heartbeat_seconds = 300
        except Exception:
            heartbeat_seconds = 30

        group = None
        if sub.is_actor_view:
            group = load_group(sub.group_id)

        for ev in _resume_candidates(group_id, since_event_id=since_event_id, since_ts=since_ts, kinds=sub.kinds):
            if _seen(str(ev.get("id") or "")):
                continue
            if sub.is_actor_view:
                kind = str(ev.get("kind") or "").strip()
                if kind not in ("chat.message", "system.notify"):
                    continue
                if kind == "chat.message" and str(ev.get("by") or "").strip() == sub.by:
                    continue
                if group is None:
                    continue
                if not is_message_for_actor(group, actor_id=sub.by, event=ev):
                    continue
            _send_ndjson(sock, {"t": "event", "event": ev})

        while True:
            try:
                item = sub.q.get(timeout=float(heartbeat_seconds))
            except queue.Empty:
                _send_ndjson(sock, {"t": "heartbeat", "ts": utc_now_iso()})
                continue

            if item is None:
                break

            if _seen(str(item.get("id") or "")):
                continue

            _send_ndjson(sock, {"t": "event", "event": item})
    except (BrokenPipeError, ConnectionResetError, OSError):
        return
    except Exception:
        return
    finally:
        EVENT_BROADCASTER.unsubscribe(sub)
        try:
            sock.close()
        except Exception:
            pass
