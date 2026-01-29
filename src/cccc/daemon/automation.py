"""Automation manager for CCCC daemon.

Automation levels:
1. Message-level: nudge (unread timeout)
2. Session-level: actor idle detection, keepalive, group silence detection, standup

All automation respects group state:
- active: All automation enabled
- idle: Automation disabled (task complete, waiting for new work)
- paused: Automation disabled (user paused)
"""
from __future__ import annotations

import json
import threading
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from ..contracts.v1 import SystemNotifyData
from ..kernel.actors import list_actors, find_foreman
from ..kernel.group import Group, load_group, get_group_state
from ..kernel.inbox import iter_events, is_message_for_actor, get_cursor
from ..kernel.ledger import append_event
from ..kernel.terminal_transcript import get_terminal_transcript_settings
from ..kernel.prompt_files import DEFAULT_STANDUP_TEMPLATE, STANDUP_FILENAME, read_repo_prompt_file
from ..runners import pty as pty_runner
from ..runners import headless as headless_runner
from .delivery import flush_pending_messages, queue_system_notify
from ..util.conv import coerce_bool
from ..util.fs import atomic_write_json, read_json
from ..util.time import parse_utc_iso, utc_now_iso


@dataclass(frozen=True)
class AutomationConfig:
    """Automation configuration for a group."""
    # Level 1: Message-level
    nudge_after_seconds: int          # Nudge actor if unread message older than this
    auto_mark_on_delivery: bool       # Auto-mark messages as read after PTY delivery

    # Level 2: Session-level
    actor_idle_timeout_seconds: int   # Notify foreman if actor idle for this long
    keepalive_delay_seconds: int      # Send keepalive after Next: declaration
    keepalive_max_per_actor: int      # Max consecutive keepalives per actor
    silence_timeout_seconds: int      # Check group if silent for this long
    standup_interval_seconds: int     # Periodic standup reminder for foreman (0 to disable)

    # Level 3: Help refresh nudges (actor-facing)
    help_nudge_interval_seconds: int  # Minimum time between help nudges per actor (0 to disable)
    help_nudge_min_messages: int      # Minimum delivered messages between help nudges (0 to disable)


def _cfg(group: Group) -> AutomationConfig:
    """Load automation config from group.yaml."""
    doc = group.doc.get("automation")
    d = doc if isinstance(doc, dict) else {}

    def _int(key: str, default: int) -> int:
        try:
            v = int(d.get(key) if key in d else default)
        except Exception:
            v = int(default)
        return max(0, v)

    return AutomationConfig(
        # Level 1
        nudge_after_seconds=_int("nudge_after_seconds", 300),
        auto_mark_on_delivery=coerce_bool(d.get("auto_mark_on_delivery"), default=False),
        # Level 2
        actor_idle_timeout_seconds=_int("actor_idle_timeout_seconds", 600),
        keepalive_delay_seconds=_int("keepalive_delay_seconds", 120),
        keepalive_max_per_actor=_int("keepalive_max_per_actor", 3),
        silence_timeout_seconds=_int("silence_timeout_seconds", 600),
        standup_interval_seconds=_int("standup_interval_seconds", 900),  # Default 15 minutes
        # Level 3
        help_nudge_interval_seconds=_int("help_nudge_interval_seconds", 600),
        help_nudge_min_messages=_int("help_nudge_min_messages", 10),
    )


def _state_path(group: Group) -> Path:
    return group.path / "state" / "automation.json"


def _load_state(group: Group) -> Dict[str, Any]:
    doc = read_json(_state_path(group))
    if not isinstance(doc, dict):
        doc = {}
    # Schema marker (best-effort). We only use it for future migrations, but keep it monotonic.
    try:
        v = int(doc.get("v") or 0)
    except Exception:
        v = 0
    if v < 4:
        doc["v"] = 4
    actors = doc.get("actors")
    if not isinstance(actors, dict):
        actors = {}
        doc["actors"] = actors
    return doc


def _save_state(group: Group, doc: Dict[str, Any]) -> None:
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(_state_path(group), doc)


def _actor_state(doc: Dict[str, Any], actor_id: str) -> Dict[str, Any]:
    actors = doc.get("actors")
    if not isinstance(actors, dict):
        actors = {}
        doc["actors"] = actors
    st = actors.get(actor_id)
    if not isinstance(st, dict):
        st = {}
        actors[actor_id] = st
    return st


def _get_last_group_activity(group: Group) -> Optional[datetime]:
    """Get timestamp of last activity in the group (any event)."""
    last_ts: Optional[datetime] = None
    for ev in iter_events(group.ledger_path):
        ts_str = str(ev.get("ts") or "")
        if ts_str:
            dt = parse_utc_iso(ts_str)
            if dt is not None:
                last_ts = dt
    return last_ts


def _get_last_actor_activity(group: Group, actor_id: str) -> Optional[datetime]:
    """Get timestamp of last activity by a specific actor."""
    last_ts: Optional[datetime] = None
    for ev in iter_events(group.ledger_path):
        by = str(ev.get("by") or "")
        if by == actor_id:
            ts_str = str(ev.get("ts") or "")
            if ts_str:
                dt = parse_utc_iso(ts_str)
                if dt is not None:
                    last_ts = dt
    return last_ts


def _terminal_tail_snippet(group: Group, *, actor_id: str, lines: int) -> str:
    """Best-effort tail snippet for notifications (compact; bounded)."""
    aid = str(actor_id or "").strip()
    if not aid:
        return ""
    try:
        if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
            return ""
    except Exception:
        return ""

    n_lines = int(lines or 0)
    if n_lines <= 0:
        n_lines = 20
    if n_lines > 80:
        n_lines = 80

    try:
        raw = pty_runner.SUPERVISOR.tail_output(group_id=group.group_id, actor_id=aid, max_bytes=200_000)
    except Exception:
        raw = b""
    raw_text = raw.decode("utf-8", errors="replace")
    if not raw_text.strip():
        return ""

    text = raw_text
    try:
        from ..util.terminal_render import render_transcript

        text = render_transcript(text, compact=True)
    except Exception:
        pass

    tail_lines = text.splitlines()[-n_lines:] if text else []
    snippet = "\n".join(tail_lines).rstrip()
    if not snippet.strip():
        return ""

    # Keep notifications bounded.
    if len(snippet) > 6000:
        snippet = snippet[-6000:]
    return snippet.rstrip()


def _actor_declared_next(group: Group, actor_id: str) -> Optional[Tuple[str, datetime]]:
    """Check if actor's last message contains 'Next:' declaration.
    
    Returns (next_text, timestamp) if found, None otherwise.
    """
    last_next: Optional[Tuple[str, datetime]] = None
    for ev in iter_events(group.ledger_path):
        if str(ev.get("kind") or "") != "chat.message":
            continue
        if str(ev.get("by") or "") != actor_id:
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        text = str(data.get("text") or "")
        # Look for "Next:" pattern
        if "Next:" in text or "next:" in text:
            ts_str = str(ev.get("ts") or "")
            dt = parse_utc_iso(ts_str)
            if dt is not None:
                # Extract the Next: content
                for line in text.split("\n"):
                    if line.strip().lower().startswith("next:"):
                        last_next = (line.strip(), dt)
                        break
    return last_next


def _queue_notify_to_pty(
    group: Group,
    *,
    actor_id: str,
    runner_kind: str,
    ev: Dict[str, Any],
    notify: SystemNotifyData,
) -> None:
    if runner_kind != "pty":
        return
    if not pty_runner.SUPERVISOR.actor_running(group.group_id, actor_id):
        return
    event_id = str(ev.get("id") or "").strip()
    if not event_id:
        return
    event_ts = str(ev.get("ts") or "").strip()
    queue_system_notify(
        group,
        actor_id=actor_id,
        event_id=event_id,
        notify_kind=str(notify.kind),
        title=str(notify.title),
        message=str(notify.message),
        ts=event_ts,
    )
    flush_pending_messages(group, actor_id=actor_id)


class AutomationManager:
    """Manages automation for all groups.
    
    Automation levels:
    1. Message-level: nudge (unread timeout)
    2. Session-level: actor idle detection, keepalive, group silence detection
    
    Automation respects group state:
    - active: All automation enabled
    - idle: Automation disabled (task complete, waiting for new work)
    - paused: Automation disabled (user paused)
    """
    
    def __init__(self) -> None:
        self._lock = threading.Lock()

    def on_resume(self, group: Group) -> None:
        """Reset automation timers on resume (idle/paused -> active).

        Design: we do not "catch up" on missed reminders; all timing starts from resume.
        """
        now = utc_now_iso()
        with self._lock:
            state = _load_state(group)
            state["resume_at"] = now
            state["last_silence_notify_at"] = now
            state["last_standup_at"] = now
            try:
                state["help_ledger_pos"] = int(group.ledger_path.stat().st_size)
            except Exception:
                pass
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                st = _actor_state(state, aid)
                st["last_idle_notify_at"] = now
                st["keepalive_count"] = 0
                st["last_keepalive_at"] = now
                st["last_nudge_event_id"] = ""
                st["last_nudge_at"] = now
                st["help_last_nudge_at"] = now
                st["help_msg_count_since"] = 0
                runner_kind = str(actor.get("runner") or "pty").strip()
                session_key: Optional[str] = None
                if runner_kind == "headless":
                    try:
                        hs = headless_runner.SUPERVISOR.get_state(group_id=group.group_id, actor_id=aid)
                        session_key = str(hs.started_at) if hs is not None else None
                    except Exception:
                        session_key = None
                else:
                    try:
                        session_key = pty_runner.SUPERVISOR.session_key(group_id=group.group_id, actor_id=aid)
                    except Exception:
                        session_key = None
                st["help_session_key"] = str(session_key or "")
            _save_state(group, state)

    def tick(self, *, home: Path) -> None:
        """Called periodically by daemon to check all groups."""
        base = home / "groups"
        if not base.exists():
            return
        for p in base.glob("*/group.yaml"):
            gid = p.parent.name
            group = load_group(gid)
            if group is None:
                continue
            if not (
                pty_runner.SUPERVISOR.group_running(gid)
                or headless_runner.SUPERVISOR.group_running(gid)
            ):
                continue
            # Check group state - skip automation if not active
            state = get_group_state(group)
            if state != "active":
                continue
            try:
                self._tick_group(group)
            except Exception:
                continue

    def _tick_group(self, group: Group) -> None:
        """Run all automation checks for a group."""
        cfg = _cfg(group)
        now = datetime.now(timezone.utc)
        
        # Level 1: Message-level checks
        self._check_nudge(group, cfg, now)
        
        # Level 2: Session-level checks
        self._check_actor_idle(group, cfg, now)
        self._check_keepalive(group, cfg, now)
        self._check_silence(group, cfg, now)
        self._check_standup(group, cfg, now)

        # Level 3: Actor-facing help nudges
        self._check_help_nudge(group, cfg, now)

    def _check_nudge(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Check for actors with old unread messages and send nudge."""
        if cfg.nudge_after_seconds <= 0:
            return

        # Pre-compute per-actor ACKs for attention messages (ledger-derived, best-effort).
        acked_by_actor: Dict[str, set[str]] = {}
        try:
            for ev in iter_events(group.ledger_path):
                if str(ev.get("kind") or "") != "chat.ack":
                    continue
                data = ev.get("data")
                if not isinstance(data, dict):
                    continue
                aid = str(data.get("actor_id") or "").strip()
                eid = str(data.get("event_id") or "").strip()
                if not aid or not eid:
                    continue
                acked_by_actor.setdefault(aid, set()).add(eid)
        except Exception:
            acked_by_actor = {}

        resume_dt: Optional[datetime] = None
        to_nudge: List[Tuple[str, List[Tuple[str, str, str]], str]] = []  # (actor_id, pending[(kind,event_id,ts)], runner_kind)

        with self._lock:
            state = _load_state(group)
            resume_dt = parse_utc_iso(str(state.get("resume_at") or "")) if state.get("resume_at") else None
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                if not coerce_bool(actor.get("enabled"), default=True):
                    continue
                # Check if actor is running
                runner_kind = str(actor.get("runner") or "pty").strip()
                if runner_kind == "headless":
                    if not headless_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue
                else:
                    if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue

                # Determine oldest unread (cursor-based) and oldest unacked IMPORTANT (ack-based).
                _, cursor_ts = get_cursor(group, aid)
                cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None

                created_ts = str(actor.get("created_at") or "").strip()
                created_dt = parse_utc_iso(created_ts) if created_ts else None

                oldest_unread_id = ""
                oldest_unread_ts = ""
                oldest_unacked_id = ""
                oldest_unacked_ts = ""

                acked_set = acked_by_actor.get(aid, set())

                for ev in iter_events(group.ledger_path):
                    kind = str(ev.get("kind") or "")
                    if kind not in ("chat.message", "system.notify"):
                        continue
                    if kind == "chat.message" and str(ev.get("by") or "") == aid:
                        # Never nudge someone about their own sent messages.
                        continue
                    if not is_message_for_actor(group, actor_id=aid, event=ev):
                        continue

                    ev_id = str(ev.get("id") or "").strip()
                    ev_ts = str(ev.get("ts") or "").strip()
                    if not ev_id or not ev_ts:
                        continue
                    ev_dt = parse_utc_iso(ev_ts)
                    if ev_dt is None:
                        continue

                    # Oldest unread: any visible event after read cursor.
                    if not oldest_unread_id:
                        if cursor_dt is None or ev_dt > cursor_dt:
                            oldest_unread_id = ev_id
                            oldest_unread_ts = ev_ts

                    # Oldest unacked IMPORTANT: visible attention chat.message, actor existed, not acked.
                    if not oldest_unacked_id and kind == "chat.message":
                        data = ev.get("data")
                        if isinstance(data, dict) and str(data.get("priority") or "normal").strip() == "attention":
                            if created_dt is None or created_dt <= ev_dt:
                                if ev_id not in acked_set:
                                    oldest_unacked_id = ev_id
                                    oldest_unacked_ts = ev_ts

                    if oldest_unread_id and oldest_unacked_id:
                        break

                # Decide which pending item should trigger this nudge (if any).
                pending: List[Tuple[datetime, str, str, str]] = []  # (base_dt, kind, id, ts)

                def _base_dt(ts: str) -> Optional[datetime]:
                    dt = parse_utc_iso(ts)
                    if dt is None:
                        return None
                    if resume_dt is not None and dt < resume_dt:
                        dt = resume_dt
                    return dt

                if oldest_unread_id and oldest_unread_ts:
                    dt = _base_dt(oldest_unread_ts)
                    if dt is not None and (now - dt).total_seconds() >= float(cfg.nudge_after_seconds):
                        pending.append((dt, "unread", oldest_unread_id, oldest_unread_ts))

                if oldest_unacked_id and oldest_unacked_ts:
                    dt = _base_dt(oldest_unacked_ts)
                    if dt is not None and (now - dt).total_seconds() >= float(cfg.nudge_after_seconds):
                        pending.append((dt, "unacked_important", oldest_unacked_id, oldest_unacked_ts))

                if not pending:
                    continue

                pending.sort(key=lambda x: x[0])
                _, _, primary_id, _ = pending[0]
                pending_info = [(k, eid, ts) for _, k, eid, ts in pending]

                st = _actor_state(state, aid)
                last_ev_id = str(st.get("last_nudge_event_id") or "")
                last_nudge_dt = parse_utc_iso(str(st.get("last_nudge_at") or "")) if st.get("last_nudge_at") else None
                # Repeat nudges while the oldest unread message stays unread.
                # - If the oldest unread changed, send immediately (once it is older than nudge_after_seconds).
                # - Otherwise, send again every nudge_after_seconds until the message is marked read.
                if last_ev_id == primary_id and last_nudge_dt is not None:
                    since_last = (now - last_nudge_dt).total_seconds()
                    if since_last < float(cfg.nudge_after_seconds):
                        continue
                st["last_nudge_event_id"] = primary_id
                st["last_nudge_at"] = utc_now_iso()
                to_nudge.append((aid, pending_info, runner_kind))

            if to_nudge:
                _save_state(group, state)

        for aid, pending_info, runner_kind in to_nudge:
            parts: List[str] = []
            title = "Unread messages waiting"
            for kind, event_id, event_ts in pending_info:
                if kind == "unacked_important":
                    title = "Important message needs acknowledgement"
                    parts.append(
                        f"IMPORTANT awaiting ACK: event_id={event_id} (since {event_ts}). "
                        f"To ACK: run cccc_inbox_mark_read(event_id={event_id}) (mark_all_read does not ACK)."
                    )
                elif kind == "unread":
                    parts.append(f"Unread messages: oldest from {event_ts}. Use cccc_inbox_list() to check.")

            notify_data = SystemNotifyData(
                kind="nudge",
                priority="normal",
                title=title,
                message="\n".join(parts),
                target_actor_id=aid,
                requires_ack=False,
            )
            ev = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=notify_data.model_dump(),
            )
            _queue_notify_to_pty(group, actor_id=aid, runner_kind=runner_kind, ev=ev, notify=notify_data)

    def _check_actor_idle(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Check for idle actors and notify foreman.

        Idle detection uses multiple signals:
        1. PTY output activity (for pty runners) - most accurate for CLI agents
        2. Ledger activity (last event by actor) - fallback for headless runners

        An actor is considered idle only if BOTH signals indicate inactivity.
        """
        if cfg.actor_idle_timeout_seconds <= 0:
            return

        foreman = find_foreman(group)
        if foreman is None:
            return  # No foreman to notify
        foreman_id = str(foreman.get("id") or "")

        to_notify: List[Tuple[str, float]] = []  # (actor_id, idle_seconds)

        with self._lock:
            state = _load_state(group)
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                if not coerce_bool(actor.get("enabled"), default=True):
                    continue
                # Skip foreman itself
                if aid == foreman_id:
                    continue
                # Check if actor is running
                runner_kind = str(actor.get("runner") or "pty").strip()
                if runner_kind == "headless":
                    if not headless_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue
                else:
                    if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue

                # Get idle time from PTY (if applicable) - this is the most accurate signal
                # for CLI-based agents that produce terminal output while working
                pty_idle_seconds: Optional[float] = None
                if runner_kind != "headless":
                    pty_idle_seconds = pty_runner.SUPERVISOR.idle_seconds(
                        group_id=group.group_id, actor_id=aid
                    )

                # Get last activity from ledger (fallback)
                last_activity = _get_last_actor_activity(group, aid)
                ledger_idle_seconds: Optional[float] = None
                if last_activity is not None:
                    ledger_idle_seconds = (now - last_activity).total_seconds()

                # Determine effective idle time:
                # - For PTY actors: use PTY idle time (more accurate)
                # - For headless: use ledger idle time
                # - If PTY shows recent activity, actor is NOT idle even if ledger is old
                if pty_idle_seconds is not None:
                    idle_seconds = pty_idle_seconds
                elif ledger_idle_seconds is not None:
                    idle_seconds = ledger_idle_seconds
                else:
                    continue  # No activity data, skip

                if idle_seconds < float(cfg.actor_idle_timeout_seconds):
                    continue

                st = _actor_state(state, aid)
                last_idle_notify = st.get("last_idle_notify_at")
                if last_idle_notify:
                    last_notify_dt = parse_utc_iso(str(last_idle_notify))
                    if last_notify_dt is not None:
                        # Don't notify again within the timeout period
                        if (now - last_notify_dt).total_seconds() < float(cfg.actor_idle_timeout_seconds):
                            continue

                st["last_idle_notify_at"] = utc_now_iso()
                to_notify.append((aid, idle_seconds))

            if to_notify:
                _save_state(group, state)

        for aid, idle_seconds in to_notify:
            tt = get_terminal_transcript_settings(group.doc)
            msg = f"Actor {aid} has been quiet for {int(idle_seconds)}s. They might be stuck or waiting for input."
            if bool(tt.get("notify_tail", False)) and str(tt.get("visibility") or "foreman") != "off":
                n_lines = int(tt.get("notify_lines") or 20)
                snippet = _terminal_tail_snippet(group, actor_id=aid, lines=n_lines)
                if snippet:
                    msg = f"{msg}\n\n---\nTerminal tail ({aid}, last {n_lines} lines):\n{snippet}"
            notify_data = SystemNotifyData(
                kind="actor_idle",
                priority="normal",
                title=f"Actor {aid} may need attention",
                message=msg,
                target_actor_id=foreman_id,
                requires_ack=False,
            )
            ev = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=notify_data.model_dump(),
            )
            foreman_runner_kind = str(foreman.get("runner") or "pty").strip()
            _queue_notify_to_pty(group, actor_id=foreman_id, runner_kind=foreman_runner_kind, ev=ev, notify=notify_data)

    def _check_keepalive(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Check for actors that declared Next: and send keepalive if needed."""
        if cfg.keepalive_delay_seconds <= 0:
            return

        resume_dt: Optional[datetime] = None
        to_keepalive: List[Tuple[str, str, str]] = []  # (actor_id, next_text, runner_kind)

        with self._lock:
            state = _load_state(group)
            resume_dt = parse_utc_iso(str(state.get("resume_at") or "")) if state.get("resume_at") else None
            for actor in list_actors(group):
                if not isinstance(actor, dict):
                    continue
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue
                if not coerce_bool(actor.get("enabled"), default=True):
                    continue
                # Check if actor is running
                runner_kind = str(actor.get("runner") or "pty").strip()
                if runner_kind == "headless":
                    if not headless_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue
                else:
                    if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue

                # Check for Next: declaration
                next_info = _actor_declared_next(group, aid)
                if next_info is None:
                    continue
                next_text, next_ts = next_info
                if resume_dt is not None and next_ts < resume_dt:
                    next_ts = resume_dt

                st = _actor_state(state, aid)
                
                # Check keepalive count
                keepalive_count = int(st.get("keepalive_count") or 0)
                last_keepalive_next = st.get("last_keepalive_next")
                last_keepalive_at = parse_utc_iso(str(st.get("last_keepalive_at") or "")) if st.get("last_keepalive_at") else None
                
                # Reset count if this is a new Next: declaration
                if last_keepalive_next != next_text:
                    keepalive_count = 0
                    st["last_keepalive_next"] = next_text
                    last_keepalive_at = None
                
                # Check max keepalives
                if keepalive_count >= cfg.keepalive_max_per_actor:
                    continue

                # Rate limit keepalive reminders. Use the same delay for the initial reminder
                # and for subsequent reminders (measured from the last keepalive we sent).
                base_dt = next_ts if keepalive_count <= 0 else (last_keepalive_at or next_ts)
                elapsed = (now - base_dt).total_seconds()
                if elapsed < float(cfg.keepalive_delay_seconds):
                    continue
                
                st["keepalive_count"] = keepalive_count + 1
                st["last_keepalive_at"] = utc_now_iso()
                to_keepalive.append((aid, next_text, runner_kind))

            if to_keepalive:
                _save_state(group, state)

        for aid, next_text, runner_kind in to_keepalive:
            notify_data = SystemNotifyData(
                kind="keepalive",
                priority="normal",
                title="Ready to continue?",
                message=f"You mentioned: '{next_text}'. Continue when ready.",
                target_actor_id=aid,
                requires_ack=False,
            )
            ev = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=notify_data.model_dump(),
            )
            _queue_notify_to_pty(group, actor_id=aid, runner_kind=runner_kind, ev=ev, notify=notify_data)

    def _check_silence(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Check if group has been silent and notify foreman."""
        if cfg.silence_timeout_seconds <= 0:
            return

        foreman = find_foreman(group)
        if foreman is None:
            return  # No foreman to notify
        foreman_id = str(foreman.get("id") or "")

        last_activity = _get_last_group_activity(group)
        if last_activity is None:
            return  # No activity yet

        silence_seconds = (now - last_activity).total_seconds()
        if silence_seconds < float(cfg.silence_timeout_seconds):
            return

        with self._lock:
            state = _load_state(group)
            last_silence_notify = state.get("last_silence_notify_at")
            if last_silence_notify:
                last_notify_dt = parse_utc_iso(str(last_silence_notify))
                if last_notify_dt is not None:
                    # Don't notify again within the timeout period
                    if (now - last_notify_dt).total_seconds() < float(cfg.silence_timeout_seconds):
                        return
            
            state["last_silence_notify_at"] = utc_now_iso()
            _save_state(group, state)

        msg = f"No activity for {int(silence_seconds)}s. Check if work is complete or if anyone needs help."

        notify_data = SystemNotifyData(
            kind="silence_check",
            priority="normal",
            title="Group is quiet",
            message=msg,
            target_actor_id=foreman_id,
            requires_ack=False,
        )
        ev = append_event(
            group.ledger_path,
            kind="system.notify",
            group_id=group.group_id,
            scope_key="",
            by="system",
            data=notify_data.model_dump(),
        )
        foreman_runner_kind = str(foreman.get("runner") or "pty").strip()
        _queue_notify_to_pty(group, actor_id=foreman_id, runner_kind=foreman_runner_kind, ev=ev, notify=notify_data)

    def _check_standup(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Check if it's time for a periodic standup meeting.
        
        Standup is a team review mechanism where foreman gathers peers to:
        1. Update progress in context
        2. Reflect on approach - is it correct? any blind spots?
        3. Share ideas and concerns
        4. Collectively decide on adjustments
        """
        if cfg.standup_interval_seconds <= 0:
            return
        
        # Find foreman - standup is only sent to foreman
        foreman = find_foreman(group)
        if foreman is None:
            return
        foreman_id = str(foreman.get("id") or "").strip()
        if not foreman_id:
            return
        
        # Check if foreman is running
        runner_kind = str(foreman.get("runner") or "pty").strip()
        if runner_kind == "headless":
            if not headless_runner.SUPERVISOR.actor_running(group.group_id, foreman_id):
                return
        else:
            if not pty_runner.SUPERVISOR.actor_running(group.group_id, foreman_id):
                return

        with self._lock:
            state = _load_state(group)
            last_standup = state.get("last_standup_at")
            
            if last_standup:
                last_standup_dt = parse_utc_iso(str(last_standup))
                if last_standup_dt is not None:
                    elapsed = (now - last_standup_dt).total_seconds()
                    if elapsed < float(cfg.standup_interval_seconds):
                        return
            
            state["last_standup_at"] = utc_now_iso()
            _save_state(group, state)

        # Calculate minutes since last standup for the message
        interval_minutes = cfg.standup_interval_seconds // 60

        pf = read_repo_prompt_file(group, STANDUP_FILENAME)
        template = str(pf.content or "").strip() if pf.found else ""
        if not template:
            template = str(DEFAULT_STANDUP_TEMPLATE or "")

        standup_message = template.replace("{{interval_minutes}}", str(interval_minutes)).strip()

        notify_data = SystemNotifyData(
            kind="standup",
            priority="normal",
            title="Stand-up Meeting",
            message=standup_message,
            target_actor_id=foreman_id,
            requires_ack=False,
        )
        ev = append_event(
            group.ledger_path,
            kind="system.notify",
            group_id=group.group_id,
            scope_key="",
            by="system",
            data=notify_data.model_dump(),
        )
        _queue_notify_to_pty(group, actor_id=foreman_id, runner_kind=runner_kind, ev=ev, notify=notify_data)

    def _check_help_nudge(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Remind running actors to refresh the help playbook via cccc_help.

        This is intentionally low-frequency and tied to "work volume" rather than pure time,
        to avoid nagging idle sessions.
        """
        if cfg.help_nudge_interval_seconds <= 0 or cfg.help_nudge_min_messages <= 0:
            return

        # Snapshot currently running actors (we only track "work volume" while running).
        running: list[tuple[str, str, str]] = []  # (actor_id, runner_kind, session_key)
        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid or aid == "user":
                continue
            if not coerce_bool(actor.get("enabled"), default=True):
                continue
            runner_kind = str(actor.get("runner") or "pty").strip()
            if runner_kind == "headless":
                if not headless_runner.SUPERVISOR.actor_running(group.group_id, aid):
                    continue
                try:
                    hs = headless_runner.SUPERVISOR.get_state(group_id=group.group_id, actor_id=aid)
                    session_key = str(hs.started_at) if hs is not None else ""
                except Exception:
                    session_key = ""
            else:
                if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
                    continue
                try:
                    session_key = str(pty_runner.SUPERVISOR.session_key(group_id=group.group_id, actor_id=aid) or "")
                except Exception:
                    session_key = ""
            running.append((aid, runner_kind, session_key))

        if not running:
            return

        running_ids = [aid for aid, _, _ in running]
        to_notify: list[tuple[str, str]] = []  # (actor_id, runner_kind)

        with self._lock:
            state = _load_state(group)
            dirty = False

            # Increment per-actor work counters by ingesting newly appended ledger events.
            #
            # We intentionally do not backfill on first run: start counting from "now" to
            # avoid catch-up bursts from historical ledgers.
            pos_key = "help_ledger_pos"
            try:
                ledger_size = int(group.ledger_path.stat().st_size)
            except Exception:
                ledger_size = 0

            raw_pos = state.get(pos_key)
            pos = int(raw_pos) if isinstance(raw_pos, int) else None
            if pos is None or pos < 0 or pos > ledger_size:
                state[pos_key] = ledger_size
                dirty = True
                pos = ledger_size
            else:
                events: list[dict[str, Any]] = []
                try:
                    with group.ledger_path.open("rb") as f:
                        f.seek(pos)
                        while True:
                            start = f.tell()
                            line = f.readline()
                            if not line:
                                break
                            if not line.endswith(b"\n"):
                                # Partial write; retry next tick.
                                f.seek(start)
                                break
                            pos = f.tell()
                            s = line.decode("utf-8", errors="replace").strip()
                            if not s:
                                continue
                            try:
                                ev = json.loads(s)
                            except Exception:
                                continue
                            if isinstance(ev, dict):
                                events.append(ev)
                except Exception:
                    events = []

                next_pos = int(pos or 0)
                if int(state.get(pos_key) or 0) != next_pos:
                    state[pos_key] = next_pos
                    dirty = True

                for ev in events:
                    kind = str(ev.get("kind") or "")
                    if kind not in ("chat.message", "system.notify"):
                        continue
                    for aid in running_ids:
                        try:
                            if not is_message_for_actor(group, actor_id=aid, event=ev):
                                continue
                        except Exception:
                            continue
                        st = _actor_state(state, aid)
                        try:
                            cur = int(st.get("help_msg_count_since") or 0)
                        except Exception:
                            cur = 0
                        st["help_msg_count_since"] = cur + 1
                        dirty = True

            # Decide which actors should be nudged.
            for aid, runner_kind, session_key in running:
                st = _actor_state(state, aid)

                # Reset per-actor counters when the session changes.
                if session_key and str(st.get("help_session_key") or "") != session_key:
                    st["help_session_key"] = session_key
                    st["help_last_nudge_at"] = utc_now_iso()
                    st["help_msg_count_since"] = 0
                    dirty = True
                    continue

                last_dt = parse_utc_iso(str(st.get("help_last_nudge_at") or "")) if st.get("help_last_nudge_at") else None
                if last_dt is None:
                    st["help_last_nudge_at"] = utc_now_iso()
                    st["help_msg_count_since"] = 0
                    dirty = True
                    continue

                elapsed = (now - last_dt).total_seconds()
                if elapsed < float(cfg.help_nudge_interval_seconds):
                    continue

                try:
                    count = int(st.get("help_msg_count_since") or 0)
                except Exception:
                    count = 0
                if count < int(cfg.help_nudge_min_messages):
                    continue

                st["help_last_nudge_at"] = utc_now_iso()
                st["help_msg_count_since"] = 0
                dirty = True
                to_notify.append((aid, runner_kind))

            if dirty:
                _save_state(group, state)

        for aid, runner_kind in to_notify:
            notify_data = SystemNotifyData(
                kind="help_nudge",
                priority="normal",
                title="Refresh collaboration rules",
                message="Run `cccc_help` now to refresh collaboration rules (ignoring will keep reminding).",
                target_actor_id=aid,
                requires_ack=False,
            )
            ev = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=notify_data.model_dump(),
            )
            _queue_notify_to_pty(group, actor_id=aid, runner_kind=runner_kind, ev=ev, notify=notify_data)

    def on_new_message(self, group: Group) -> None:
        """Called when a new message arrives.
        
        Note: Auto-transition from idle -> active is handled by the daemon
        message-ingest path (send/reply) for human-originated messages.
        """
        pass
