"""Automation manager for CCCC daemon.

Automation levels:
1. Message-level: nudge (unread timeout)
2. Session-level: actor idle detection, keepalive, group silence detection
3. Rule-level: user-defined automation rules (scheduled system notifications)

All automation respects group state:
- active: All automation enabled (Level 1-4)
- idle: Only user-defined rules (Level 4) run; internal automation (Level 1-3) disabled
- paused: All automation disabled
"""
from __future__ import annotations

import json
import re
import threading
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from zoneinfo import ZoneInfo

from ..contracts.v1 import AutomationRule, AutomationRuleSet, SystemNotifyData
from ..kernel.actors import list_actors, find_foreman
from ..kernel.group import Group, load_group, get_group_state
from ..kernel.inbox import iter_events, is_message_for_actor, get_cursor, get_obligation_status_batch
from ..kernel.ledger import append_event
from ..kernel.terminal_transcript import get_terminal_transcript_settings
from ..kernel.messaging import enabled_recipient_actor_ids
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
    nudge_after_seconds: int          # Global fallback nudge interval (legacy)
    reply_required_nudge_after_seconds: int  # Nudge for required-reply obligations
    attention_ack_nudge_after_seconds: int   # Nudge for attention ack obligations
    unread_nudge_after_seconds: int          # Nudge for plain unread backlog
    nudge_digest_min_interval_seconds: int   # Min interval between digest nudges per actor
    nudge_max_repeats_per_obligation: int    # Max repeats per obligation item
    nudge_escalate_after_repeats: int        # Escalate to foreman at/after this repeat count

    # Level 2: Session-level
    actor_idle_timeout_seconds: int   # Notify foreman if actor idle for this long
    keepalive_delay_seconds: int      # Send keepalive after Next: declaration
    keepalive_max_per_actor: int      # Max consecutive keepalives per actor
    silence_timeout_seconds: int      # Check group if silent for this long

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
        reply_required_nudge_after_seconds=_int("reply_required_nudge_after_seconds", _int("nudge_after_seconds", 300)),
        attention_ack_nudge_after_seconds=_int("attention_ack_nudge_after_seconds", max(1, _int("nudge_after_seconds", 300) * 2)),
        unread_nudge_after_seconds=_int("unread_nudge_after_seconds", max(1, _int("nudge_after_seconds", 300) * 3)),
        nudge_digest_min_interval_seconds=_int("nudge_digest_min_interval_seconds", 120),
        nudge_max_repeats_per_obligation=_int("nudge_max_repeats_per_obligation", 3),
        nudge_escalate_after_repeats=_int("nudge_escalate_after_repeats", 2),
        # Level 2
        actor_idle_timeout_seconds=_int("actor_idle_timeout_seconds", 600),
        keepalive_delay_seconds=_int("keepalive_delay_seconds", 120),
        keepalive_max_per_actor=_int("keepalive_max_per_actor", 3),
        silence_timeout_seconds=_int("silence_timeout_seconds", 600),
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
    if v < 5:
        doc["v"] = 5
    actors = doc.get("actors")
    if not isinstance(actors, dict):
        actors = {}
        doc["actors"] = actors
    rules = doc.get("rules")
    if not isinstance(rules, dict):
        rules = {}
        doc["rules"] = rules
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


def _rule_state(doc: Dict[str, Any], rule_id: str) -> Dict[str, Any]:
    rules = doc.get("rules")
    if not isinstance(rules, dict):
        rules = {}
        doc["rules"] = rules
    st = rules.get(rule_id)
    if not isinstance(st, dict):
        st = {}
        rules[rule_id] = st
    return st


def _load_ruleset(group: Group) -> AutomationRuleSet:
    doc = group.doc.get("automation")
    d = doc if isinstance(doc, dict) else {}

    raw_snippets = d.get("snippets")
    snippets_in = raw_snippets if isinstance(raw_snippets, dict) else {}
    snippets: Dict[str, str] = {}
    for k, v in snippets_in.items():
        if not isinstance(k, str):
            continue
        key = k.strip()
        if not key:
            continue
        if not isinstance(v, str):
            continue
        snippets[key] = v

    raw_rules = d.get("rules")
    rules_in = raw_rules if isinstance(raw_rules, list) else []
    seen: set[str] = set()
    rules: List[AutomationRule] = []
    for rr in rules_in:
        if not isinstance(rr, dict):
            continue
        try:
            rule = AutomationRule.model_validate(rr)
        except Exception:
            continue
        rid = str(rule.id or "").strip()
        if not rid or rid in seen:
            continue
        seen.add(rid)
        rules.append(rule)

    return AutomationRuleSet(rules=rules, snippets=snippets)


_SNIPPET_VAR_RE = re.compile(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}")
_AUTOMATION_SUPPORTED_VARS = ["interval_minutes", "group_title", "actor_names", "scheduled_at"]


def _render_snippet(text: str, *, context: Dict[str, str]) -> str:
    def _one(m: re.Match[str]) -> str:
        key = str(m.group(1) or "").strip()
        if not key:
            return ""
        return str(context.get(key, ""))

    return _SNIPPET_VAR_RE.sub(_one, str(text or ""))


def _actor_display_names(group: Group) -> str:
    names: List[str] = []
    for a in list_actors(group):
        if not isinstance(a, dict):
            continue
        if not coerce_bool(a.get("enabled"), default=True):
            continue
        aid = str(a.get("id") or "").strip()
        if not aid or aid == "user":
            continue
        title = str(a.get("title") or "").strip()
        names.append(title or aid)
    return ", ".join(names)


@dataclass(frozen=True)
class _CronSpec:
    minutes: set[int]
    hours: set[int]
    days_of_month: set[int]
    months: set[int]
    days_of_week: set[int]
    dom_any: bool
    dow_any: bool


def automation_supported_vars() -> List[str]:
    return list(_AUTOMATION_SUPPORTED_VARS)


def _iso_utc(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def _parse_int_in_range(raw: str, *, min_v: int, max_v: int, field_name: str) -> int:
    try:
        n = int(str(raw).strip())
    except Exception as e:
        raise ValueError(f"invalid {field_name} value: {raw}") from e
    if n < min_v or n > max_v:
        raise ValueError(f"{field_name} out of range: {n} (expected {min_v}-{max_v})")
    return n


def _parse_cron_field(
    expr: str,
    *,
    min_v: int,
    max_v: int,
    field_name: str,
    allow_7_to_0: bool = False,
) -> Tuple[set[int], bool]:
    raw = str(expr or "").strip()
    if not raw:
        raise ValueError(f"empty cron field: {field_name}")

    full_any = raw == "*"
    out: set[int] = set()

    for part in raw.split(","):
        token = str(part or "").strip()
        if not token:
            raise ValueError(f"invalid cron token in {field_name}: {raw}")

        step = 1
        base = token
        if "/" in token:
            base, step_raw = token.split("/", 1)
            step = _parse_int_in_range(step_raw, min_v=1, max_v=100_000, field_name=f"{field_name}.step")
            base = str(base or "").strip()
            if not base:
                raise ValueError(f"invalid cron step token in {field_name}: {token}")

        if base == "*":
            start, end = min_v, max_v
        elif "-" in base:
            a_raw, b_raw = base.split("-", 1)
            start = _parse_int_in_range(a_raw, min_v=min_v, max_v=max_v, field_name=field_name)
            end = _parse_int_in_range(b_raw, min_v=min_v, max_v=max_v, field_name=field_name)
            if end < start:
                raise ValueError(f"invalid cron range in {field_name}: {base}")
        else:
            n = _parse_int_in_range(base, min_v=min_v, max_v=max_v, field_name=field_name)
            start, end = n, n

        for n in range(start, end + 1, step):
            if allow_7_to_0 and n == 7:
                out.add(0)
            else:
                out.add(n)

    if not out:
        raise ValueError(f"empty cron set in {field_name}")
    return out, full_any


def _compile_cron(expr: str) -> _CronSpec:
    parts = str(expr or "").strip().split()
    if len(parts) != 5:
        raise ValueError("cron must have 5 fields: min hour dom month dow")

    minutes, _ = _parse_cron_field(parts[0], min_v=0, max_v=59, field_name="minute")
    hours, _ = _parse_cron_field(parts[1], min_v=0, max_v=23, field_name="hour")
    dom, dom_any = _parse_cron_field(parts[2], min_v=1, max_v=31, field_name="day_of_month")
    months, _ = _parse_cron_field(parts[3], min_v=1, max_v=12, field_name="month")
    dow, dow_any = _parse_cron_field(parts[4], min_v=0, max_v=7, field_name="day_of_week", allow_7_to_0=True)

    return _CronSpec(
        minutes=minutes,
        hours=hours,
        days_of_month=dom,
        months=months,
        days_of_week=dow,
        dom_any=dom_any,
        dow_any=dow_any,
    )


def _cron_matches(spec: _CronSpec, local_dt: datetime) -> bool:
    if local_dt.minute not in spec.minutes:
        return False
    if local_dt.hour not in spec.hours:
        return False
    if local_dt.month not in spec.months:
        return False

    day_of_month_match = local_dt.day in spec.days_of_month
    day_of_week = (local_dt.weekday() + 1) % 7  # Sunday=0, Monday=1...
    day_of_week_match = day_of_week in spec.days_of_week

    if spec.dom_any and spec.dow_any:
        return True
    if spec.dom_any:
        return day_of_week_match
    if spec.dow_any:
        return day_of_month_match
    return day_of_month_match or day_of_week_match


def _cron_next_fire_utc(*, cron_expr: str, tz_name: str, now_utc: datetime) -> Optional[datetime]:
    spec = _compile_cron(cron_expr)
    tz = ZoneInfo(str(tz_name or "UTC"))
    now_local = now_utc.astimezone(tz)
    cursor = now_local.replace(second=0, microsecond=0)
    if now_local > cursor:
        cursor = cursor + timedelta(minutes=1)

    for _ in range(366 * 24 * 60):
        if _cron_matches(spec, cursor):
            return cursor.astimezone(timezone.utc)
        cursor = cursor + timedelta(minutes=1)
    return None


def _rule_next_fire_at(rule: AutomationRule, rule_state: Dict[str, Any], *, now_utc: datetime) -> Optional[datetime]:
    if not bool(rule.enabled):
        return None
    trigger = rule.trigger

    if trigger.kind == "interval":
        every_seconds = int(trigger.every_seconds or 0)
        if every_seconds <= 0:
            return None
        last_dt = parse_utc_iso(str(rule_state.get("last_fired_at") or ""))
        if last_dt is None:
            return now_utc + timedelta(seconds=every_seconds)
        return last_dt + timedelta(seconds=every_seconds)

    if trigger.kind == "cron":
        try:
            return _cron_next_fire_utc(cron_expr=str(trigger.cron or ""), tz_name=str(trigger.timezone or "UTC"), now_utc=now_utc)
        except Exception:
            return None

    if trigger.kind == "at":
        if coerce_bool(rule_state.get("at_fired"), default=False):
            return None
        at_dt = parse_utc_iso(str(trigger.at or ""))
        if at_dt is None:
            return None
        return at_dt

    return None


def build_automation_status(group: Group, *, now: Optional[datetime] = None) -> Dict[str, Dict[str, Any]]:
    now_utc = now.astimezone(timezone.utc) if isinstance(now, datetime) else datetime.now(timezone.utc)
    ruleset = _load_ruleset(group)
    state = _load_state(group)
    rules_state = state.get("rules") if isinstance(state.get("rules"), dict) else {}

    out: Dict[str, Dict[str, Any]] = {}
    for rule in ruleset.rules:
        rid = str(rule.id or "").strip()
        if not rid:
            continue
        st = rules_state.get(rid) if isinstance(rules_state, dict) else None
        st_dict = st if isinstance(st, dict) else {}
        next_fire = _rule_next_fire_at(rule, st_dict, now_utc=now_utc)
        completed = False
        completed_at = ""
        try:
            if str(getattr(rule.trigger, "kind", "") or "").strip() == "at" and coerce_bool(st_dict.get("at_fired"), default=False):
                completed = True
                completed_at = str(st_dict.get("last_fired_at") or "")
        except Exception:
            completed = False
            completed_at = ""
        out[rid] = {
            "last_fired_at": str(st_dict.get("last_fired_at") or ""),
            "last_error_at": str(st_dict.get("last_error_at") or ""),
            "last_error": str(st_dict.get("last_error") or ""),
            "next_fire_at": _iso_utc(next_fire) if next_fire is not None else "",
            "completed": bool(completed),
            "completed_at": str(completed_at or ""),
        }
    return out


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


def _nudge_item_repeat_count(state: Dict[str, Any], actor_id: str, item_key: str) -> int:
    st = _actor_state(state, actor_id)
    items = st.get("nudge_items") if isinstance(st.get("nudge_items"), dict) else {}
    rec = items.get(item_key) if isinstance(items, dict) else None
    if not isinstance(rec, dict):
        return 0
    try:
        return max(0, int(rec.get("count") or 0))
    except Exception:
        return 0


def _nudge_item_touch(state: Dict[str, Any], actor_id: str, item_key: str) -> int:
    st = _actor_state(state, actor_id)
    items = st.get("nudge_items")
    if not isinstance(items, dict):
        items = {}
        st["nudge_items"] = items
    rec = items.get(item_key)
    if not isinstance(rec, dict):
        rec = {"count": 0}
        items[item_key] = rec
    try:
        count = int(rec.get("count") or 0)
    except Exception:
        count = 0
    count = max(0, count) + 1
    rec["count"] = count
    rec["last_nudged_at"] = utc_now_iso()
    return count


def _nudge_items_gc(state: Dict[str, Any], actor_id: str, alive_keys: set[str]) -> None:
    st = _actor_state(state, actor_id)
    items = st.get("nudge_items")
    if not isinstance(items, dict):
        return
    for k in list(items.keys()):
        if k not in alive_keys:
            items.pop(k, None)


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
            try:
                state["help_ledger_pos"] = int(group.ledger_path.stat().st_size)
            except Exception:
                pass
            # Reset user-defined automation rule timers to avoid "catch up" bursts.
            ruleset = _load_ruleset(group)
            for rule in ruleset.rules:
                rid = str(rule.id or "").strip()
                if not rid:
                    continue
                st_rule = _rule_state(state, rid)
                st_rule["last_fired_at"] = now
                st_rule["last_error_at"] = ""
                st_rule["last_error"] = ""
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
                st["nudge_items"] = {}
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
            # Check group state - gate automation by state
            state = get_group_state(group)
            if state == "paused":
                continue  # paused: all automation disabled
            if state == "idle":
                # idle: only run user-defined rules (Level 4);
                # internal automation (Level 1-3) stays silent
                try:
                    now = datetime.now(timezone.utc)
                    self._check_rules(group, now)
                except Exception:
                    pass
                continue
            # active: run all automation (Level 1-4)
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

        # Level 3: Actor-facing help nudges
        self._check_help_nudge(group, cfg, now)

        # Level 4: User-defined automation rules
        self._check_rules(group, now)

    def _check_nudge(self, group: Group, cfg: AutomationConfig, now: datetime) -> None:
        """Check pending obligations/unread and send one digest nudge per actor."""
        if (
            cfg.reply_required_nudge_after_seconds <= 0
            and cfg.attention_ack_nudge_after_seconds <= 0
            and cfg.unread_nudge_after_seconds <= 0
            and cfg.nudge_after_seconds <= 0
        ):
            return
        min_interval = max(0, int(cfg.nudge_digest_min_interval_seconds))

        try:
            roster = [
                a
                for a in list_actors(group)
                if isinstance(a, dict)
                and str(a.get("id") or "").strip()
                and coerce_bool(a.get("enabled"), default=True)
            ]
        except Exception:
            roster = []
        if not roster:
            return

        # Scan visible chat/system events once and compute obligation status in batch.
        all_events: List[Dict[str, Any]] = []
        chat_events: List[Dict[str, Any]] = []
        for ev in iter_events(group.ledger_path):
            kind = str(ev.get("kind") or "")
            if kind not in ("chat.message", "system.notify"):
                continue
            all_events.append(ev)
            if kind == "chat.message":
                chat_events.append(ev)

        obligation_map = get_obligation_status_batch(group, chat_events)

        resume_dt: Optional[datetime] = None
        to_nudge: List[Tuple[str, str, str, List[str], bool]] = []
        # (actor_id, runner_kind, title, lines, escalate)

        with self._lock:
            state = _load_state(group)
            resume_dt = parse_utc_iso(str(state.get("resume_at") or "")) if state.get("resume_at") else None
            foreman = find_foreman(group)
            foreman_id = str((foreman or {}).get("id") or "").strip() if isinstance(foreman, dict) else ""

            for actor in roster:
                aid = str(actor.get("id") or "").strip()
                if not aid:
                    continue

                runner_kind = str(actor.get("runner") or "pty").strip()
                if runner_kind == "headless":
                    if not headless_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue
                else:
                    if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
                        continue

                _, cursor_ts = get_cursor(group, aid)
                cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None

                pending_reply_required: List[Tuple[str, str]] = []
                pending_attention_ack: List[Tuple[str, str]] = []
                oldest_unread_ts = ""

                alive_item_keys: set[str] = set()
                due_item_keys: List[str] = []
                reply_due_keys: set[str] = set()
                item_lines: List[str] = []
                escalate = False

                for ev in all_events:
                    kind = str(ev.get("kind") or "")
                    if kind == "chat.message" and str(ev.get("by") or "") == aid:
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
                    base_dt = ev_dt
                    if resume_dt is not None and base_dt < resume_dt:
                        base_dt = resume_dt

                    if not oldest_unread_ts and (cursor_dt is None or ev_dt > cursor_dt):
                        oldest_unread_ts = ev_ts

                    if kind != "chat.message":
                        continue

                    status_by_recipient = obligation_map.get(ev_id) if isinstance(obligation_map.get(ev_id), dict) else {}
                    st = status_by_recipient.get(aid) if isinstance(status_by_recipient, dict) else None
                    if not isinstance(st, dict):
                        continue

                    is_reply_required = bool(st.get("reply_required") is True)
                    is_replied = bool(st.get("replied") is True)
                    is_acked = bool(st.get("acked") is True)

                    data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
                    is_attention = str(data.get("priority") or "normal").strip() == "attention"

                    if is_reply_required and not is_replied:
                        pending_reply_required.append((ev_id, ev_ts))
                        item_key = f"reply_required:{ev_id}"
                        alive_item_keys.add(item_key)
                        repeat = _nudge_item_repeat_count(state, aid, item_key)
                        if cfg.nudge_max_repeats_per_obligation > 0 and repeat >= int(cfg.nudge_max_repeats_per_obligation):
                            continue
                        due_after = max(0, int(cfg.reply_required_nudge_after_seconds))
                        if (now - base_dt).total_seconds() < float(due_after):
                            continue
                        due_item_keys.append(item_key)
                        reply_due_keys.add(item_key)
                        item_lines.append(
                            f"REPLY REQUIRED: event_id={ev_id} (since {ev_ts}). Reply via cccc_message_reply(event_id={ev_id}, ...)."
                        )
                        continue

                    if is_attention and not is_acked:
                        pending_attention_ack.append((ev_id, ev_ts))
                        item_key = f"attention_ack:{ev_id}"
                        alive_item_keys.add(item_key)
                        repeat = _nudge_item_repeat_count(state, aid, item_key)
                        if cfg.nudge_max_repeats_per_obligation > 0 and repeat >= int(cfg.nudge_max_repeats_per_obligation):
                            continue
                        due_after = max(0, int(cfg.attention_ack_nudge_after_seconds))
                        if (now - base_dt).total_seconds() < float(due_after):
                            continue
                        due_item_keys.append(item_key)
                        item_lines.append(
                            f"IMPORTANT awaiting ACK: event_id={ev_id} (since {ev_ts}). Use cccc_inbox_mark_read(event_id={ev_id})."
                        )

                # Track unread backlog as one virtual item.
                if oldest_unread_ts:
                    unread_dt = parse_utc_iso(oldest_unread_ts)
                    if unread_dt is not None:
                        base_dt = unread_dt
                        if resume_dt is not None and base_dt < resume_dt:
                            base_dt = resume_dt
                        item_key = "unread_backlog"
                        alive_item_keys.add(item_key)
                        repeat = _nudge_item_repeat_count(state, aid, item_key)
                        if cfg.nudge_max_repeats_per_obligation <= 0 or repeat < int(cfg.nudge_max_repeats_per_obligation):
                            due_after = max(0, int(cfg.unread_nudge_after_seconds))
                            if (now - base_dt).total_seconds() >= float(due_after):
                                due_item_keys.append(item_key)
                                item_lines.append(
                                    f"Unread backlog: oldest from {oldest_unread_ts}. Use cccc_inbox_list() to review."
                                )

                _nudge_items_gc(state, aid, alive_item_keys)

                if not item_lines:
                    continue

                st_actor = _actor_state(state, aid)
                last_nudge_dt = parse_utc_iso(str(st_actor.get("last_nudge_at") or "")) if st_actor.get("last_nudge_at") else None
                if last_nudge_dt is not None and min_interval > 0:
                    if (now - last_nudge_dt).total_seconds() < float(min_interval):
                        continue

                for item_key in dict.fromkeys(due_item_keys):
                    ncnt = _nudge_item_touch(state, aid, item_key)
                    if (
                        item_key in reply_due_keys
                        and ncnt >= max(1, int(cfg.nudge_escalate_after_repeats))
                        and foreman_id
                        and foreman_id != aid
                    ):
                        escalate = True

                st_actor["last_nudge_at"] = utc_now_iso()
                st_actor["last_nudge_event_id"] = "digest"

                title = "Action items pending"
                prefix: List[str] = []
                if pending_reply_required:
                    prefix.append(f"reply_required={len(pending_reply_required)}")
                if pending_attention_ack:
                    prefix.append(f"attention_ack={len(pending_attention_ack)}")
                if oldest_unread_ts:
                    prefix.append("unread>0")
                if prefix:
                    title = "Action items pending (" + ", ".join(prefix) + ")"

                to_nudge.append((aid, runner_kind, title, item_lines, escalate and bool(foreman_id)))

            if to_nudge:
                _save_state(group, state)

        for aid, runner_kind, title, item_lines, escalate in to_nudge:
            max_lines = 5
            lines = item_lines[:max_lines]
            if len(item_lines) > max_lines:
                lines.append(f"... and {len(item_lines) - max_lines} more pending item(s).")

            notify_data = SystemNotifyData(
                kind="nudge",
                priority="normal",
                title=title,
                message="\n".join(lines),
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

            if escalate:
                foreman = find_foreman(group)
                foreman_id = str((foreman or {}).get("id") or "").strip() if isinstance(foreman, dict) else ""
                if foreman_id and foreman_id != aid:
                    escalate_notify = SystemNotifyData(
                        kind="nudge",
                        priority="normal",
                        title="Escalation: pending replies",
                        message=f"{aid} has repeated pending obligations. Please intervene if needed.",
                        target_actor_id=foreman_id,
                        requires_ack=False,
                    )
                    ev2 = append_event(
                        group.ledger_path,
                        kind="system.notify",
                        group_id=group.group_id,
                        scope_key="",
                        by="system",
                        data=escalate_notify.model_dump(),
                    )
                    _queue_notify_to_pty(group, actor_id=foreman_id, runner_kind=str((foreman or {}).get("runner") or "pty"), ev=ev2, notify=escalate_notify)

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
            if coerce_bool(tt.get("notify_tail"), default=False) and str(tt.get("visibility") or "foreman") != "off":
                try:
                    n_lines = int(tt.get("notify_lines") or 20)
                except Exception:
                    n_lines = 20
                n_lines = max(1, min(80, n_lines))
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

    def _daemon_automation_call(self, *, op: str, args: Dict[str, Any]) -> Tuple[bool, str]:
        """Invoke daemon ops from automation thread without duplicating server logic."""
        try:
            from ..contracts.v1 import DaemonRequest
            from .server import handle_request

            req = DaemonRequest(op=op, args=args)
            resp, _ = handle_request(req)
        except Exception as e:
            return False, str(e)
        if bool(resp.ok):
            return True, ""
        err = resp.error
        msg = str(getattr(err, "message", "") or f"{op} failed")
        return False, msg

    def _resolve_actor_control_targets(self, group: Group, targets: List[str]) -> List[str]:
        actors = list_actors(group)
        actor_ids: List[str] = []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if aid and aid != "user":
                actor_ids.append(aid)
        if not actor_ids:
            return []

        foreman = find_foreman(group)
        foreman_id = str(foreman.get("id") or "").strip() if isinstance(foreman, dict) else ""
        peers = [aid for aid in actor_ids if aid and aid != foreman_id]

        selected: set[str] = set()
        for token in targets:
            t = str(token or "").strip()
            if not t:
                continue
            if t == "@all":
                selected.update(actor_ids)
            elif t == "@foreman":
                if foreman_id:
                    selected.add(foreman_id)
            elif t == "@peers":
                selected.update(peers)
            elif t in actor_ids:
                selected.add(t)

        if not selected:
            return []
        return [aid for aid in actor_ids if aid in selected]

    def _execute_group_state_action(self, group: Group, *, target_state: str) -> Tuple[bool, str]:
        state = str(target_state or "").strip().lower()
        if state not in ("active", "idle", "paused", "stopped"):
            return False, f"unsupported group state: {target_state}"

        if state == "stopped":
            return self._daemon_automation_call(
                op="group_stop",
                args={"group_id": group.group_id, "by": "user"},
            )

        group_now = load_group(group.group_id)
        running = coerce_bool(group_now.doc.get("running"), default=False) if group_now is not None else False
        if state == "active" and not running:
            ok, err = self._daemon_automation_call(
                op="group_start",
                args={"group_id": group.group_id, "by": "user"},
            )
            if not ok:
                return False, err
        return self._daemon_automation_call(
            op="group_set_state",
            args={"group_id": group.group_id, "state": state, "by": "user"},
        )

    def _execute_actor_control_action(
        self,
        group: Group,
        *,
        operation: str,
        targets: List[str],
    ) -> Tuple[bool, str]:
        op = str(operation or "").strip().lower()
        op_map = {
            "start": "actor_start",
            "stop": "actor_stop",
            "restart": "actor_restart",
        }
        daemon_op = op_map.get(op)
        if not daemon_op:
            return False, f"unsupported actor operation: {operation}"

        actor_ids = self._resolve_actor_control_targets(group, targets)
        if not actor_ids:
            return False, "no actor targets resolved"

        success_count = 0
        errors: List[str] = []
        for aid in actor_ids:
            ok, err = self._daemon_automation_call(
                op=daemon_op,
                args={"group_id": group.group_id, "actor_id": aid, "by": "user"},
            )
            if ok:
                success_count += 1
                continue
            if err:
                errors.append(f"{aid}: {err}")

        if success_count > 0:
            return True, ""
        if errors:
            return False, " ; ".join(errors[:3])
        return False, "no actor operations applied"

    def _check_rules(self, group: Group, now: datetime) -> None:
        """Run user-defined automation rules (scheduled system notifications)."""
        ruleset = _load_ruleset(group)
        if not ruleset.rules:
            return

        # Snapshot roster once.
        roster: Dict[str, Dict[str, Any]] = {}
        for a in list_actors(group):
            if not isinstance(a, dict):
                continue
            aid = str(a.get("id") or "").strip()
            if not aid:
                continue
            roster[aid] = a

        group_title = str(group.doc.get("title") or "").strip()
        actor_names = _actor_display_names(group)
        now_iso = _iso_utc(now)

        due: List[Dict[str, Any]] = []
        with self._lock:
            state = _load_state(group)
            dirty = False

            for rule in ruleset.rules:
                rid = str(rule.id or "").strip()
                if not rid or not bool(rule.enabled):
                    continue

                st = _rule_state(state, rid)
                trigger_kind = rule.trigger.kind
                scheduled_at = ""
                slot_key = ""
                interval_seconds = 0

                def _record_error(message: str) -> None:
                    nonlocal dirty
                    msg = str(message or "").strip()[:500]
                    if not msg:
                        return
                    if str(st.get("last_error") or "") == msg:
                        return
                    st["last_error_at"] = now_iso
                    st["last_error"] = msg
                    dirty = True

                if trigger_kind == "interval":
                    interval_seconds = int(getattr(rule.trigger, "every_seconds", 0) or 0)
                    if interval_seconds <= 0:
                        continue
                    last_dt = parse_utc_iso(str(st.get("last_fired_at") or ""))
                    if last_dt is None:
                        # New interval rule: start counting from now (no immediate fire).
                        st["last_fired_at"] = now_iso
                        dirty = True
                        continue
                    elapsed = (now - last_dt).total_seconds()
                    if elapsed < float(interval_seconds):
                        continue
                    scheduled_at = _iso_utc(last_dt + timedelta(seconds=interval_seconds))

                elif trigger_kind == "cron":
                    cron_expr = str(getattr(rule.trigger, "cron", "") or "").strip()
                    tz_name = str(getattr(rule.trigger, "timezone", "UTC") or "UTC").strip() or "UTC"
                    try:
                        cron_spec = _compile_cron(cron_expr)
                        tz = ZoneInfo(tz_name)
                    except Exception as e:
                        _record_error(f"invalid cron trigger: {e}")
                        continue

                    local_now = now.astimezone(tz)
                    slot_local = local_now.replace(second=0, microsecond=0)
                    if not _cron_matches(cron_spec, slot_local):
                        continue
                    slot_utc = slot_local.astimezone(timezone.utc)
                    slot_key = f"cron:{_iso_utc(slot_utc)}"
                    if str(st.get("last_slot_key") or "") == slot_key:
                        continue
                    # Mark slot before delivery to avoid per-second re-evaluation in the same minute.
                    st["last_slot_key"] = slot_key
                    dirty = True
                    scheduled_at = _iso_utc(slot_utc)

                elif trigger_kind == "at":
                    at_dt = parse_utc_iso(str(getattr(rule.trigger, "at", "") or ""))
                    if at_dt is None:
                        _record_error("invalid at trigger: expected RFC3339 timestamp")
                        continue
                    if coerce_bool(st.get("at_fired"), default=False):
                        continue
                    if now < at_dt:
                        continue
                    slot_key = f"at:{_iso_utc(at_dt)}"
                    scheduled_at = _iso_utc(at_dt)
                else:
                    continue

                action_kind = str(getattr(rule.action, "kind", "notify") or "notify").strip()
                if action_kind in ("group_state", "actor_control") and trigger_kind != "at":
                    _record_error(f"invalid schedule: action.kind={action_kind} only supports one-time schedules")
                    continue
                if action_kind == "notify":
                    snippet_ref = str(getattr(rule.action, "snippet_ref", "") or "").strip()
                    template = str(ruleset.snippets.get(snippet_ref, "") or "") if snippet_ref else ""
                    if not template:
                        template = str(getattr(rule.action, "message", "") or "")
                    template = str(template or "").strip()
                    if not template:
                        continue

                    ctx: Dict[str, str] = {
                        "interval_minutes": str(max(1, interval_seconds // 60)) if interval_seconds >= 60 else "0",
                        "group_title": group_title,
                        "actor_names": actor_names,
                        "scheduled_at": scheduled_at,
                    }
                    rendered = _render_snippet(template, context=ctx).strip()
                    if not rendered:
                        continue

                    to = [str(x).strip() for x in (rule.to or []) if isinstance(x, str) and str(x).strip()]
                    recipient_ids = enabled_recipient_actor_ids(group, to)
                    if not recipient_ids:
                        continue

                    due.append(
                        {
                            "rule_id": rid,
                            "rule": rule,
                            "trigger_kind": trigger_kind,
                            "slot_key": slot_key,
                            "rendered": rendered,
                            "recipient_ids": recipient_ids,
                        }
                    )
                    continue

                if action_kind in ("group_state", "actor_control"):
                    due.append(
                        {
                            "rule_id": rid,
                            "rule": rule,
                            "trigger_kind": trigger_kind,
                            "slot_key": slot_key,
                        }
                    )
                    continue

                _record_error(f"unsupported action kind: {action_kind}")

            if dirty:
                _save_state(group, state)

        if not due:
            return

        results: Dict[str, Tuple[bool, str, str, str]] = {}  # rule_id -> (sent_any, last_error, trigger_kind, slot_key)
        for item in due:
            rid = str(item.get("rule_id") or "").strip()
            rule = item.get("rule")
            if not rid or not isinstance(rule, AutomationRule):
                continue
            trigger_kind = str(item.get("trigger_kind") or "")
            slot_key = str(item.get("slot_key") or "")
            sent_any = False
            last_error = ""
            action_kind = str(getattr(rule.action, "kind", "notify") or "notify").strip()
            if action_kind == "notify":
                recipient_ids = item.get("recipient_ids") if isinstance(item.get("recipient_ids"), list) else []
                rendered = str(item.get("rendered") or "")
                for aid in recipient_ids:
                    actor = roster.get(str(aid))
                    if not isinstance(actor, dict):
                        continue
                    runner_kind = str(actor.get("runner") or "pty").strip()
                    try:
                        running = (
                            headless_runner.SUPERVISOR.actor_running(group.group_id, str(aid))
                            if runner_kind == "headless"
                            else pty_runner.SUPERVISOR.actor_running(group.group_id, str(aid))
                        )
                    except Exception:
                        running = False
                    if not running:
                        continue

                    title = str(getattr(rule.action, "title", "") or "").strip() or "Reminder"
                    notify_data = SystemNotifyData(
                        kind="automation",
                        priority=rule.action.priority,
                        title=title,
                        message=rendered,
                        target_actor_id=str(aid),
                        context={"rule_id": rid},
                        requires_ack=bool(getattr(rule.action, "requires_ack", False)),
                    )
                    try:
                        ev = append_event(
                            group.ledger_path,
                            kind="system.notify",
                            group_id=group.group_id,
                            scope_key="",
                            by="system",
                            data=notify_data.model_dump(),
                        )
                        _queue_notify_to_pty(group, actor_id=str(aid), runner_kind=runner_kind, ev=ev, notify=notify_data)
                        sent_any = True
                    except Exception as e:
                        last_error = str(e)
            elif action_kind == "group_state":
                state_target = str(getattr(rule.action, "state", "") or "").strip()
                sent_any, last_error = self._execute_group_state_action(group, target_state=state_target)
            elif action_kind == "actor_control":
                operation = str(getattr(rule.action, "operation", "") or "").strip()
                raw_targets = getattr(rule.action, "targets", [])
                targets = [str(x).strip() for x in (raw_targets or []) if isinstance(x, str) and str(x).strip()]
                sent_any, last_error = self._execute_actor_control_action(
                    group,
                    operation=operation,
                    targets=targets,
                )
            else:
                last_error = f"unsupported action kind: {action_kind}"

            results[rid] = (sent_any, last_error[:500] if last_error else "", trigger_kind, slot_key)

        # Persist rule fire/error state.
        with self._lock:
            state = _load_state(group)
            dirty = False
            for rid, (sent_any, last_error, trigger_kind, slot_key) in results.items():
                st = _rule_state(state, rid)
                if sent_any:
                    st["last_fired_at"] = now_iso
                    st["last_error_at"] = ""
                    st["last_error"] = ""
                    if trigger_kind == "at":
                        st["at_fired"] = True
                        st["last_slot_key"] = slot_key
                    elif trigger_kind == "cron" and slot_key:
                        st["last_slot_key"] = slot_key
                    dirty = True
                elif last_error:
                    st["last_error_at"] = now_iso
                    st["last_error"] = last_error
                    dirty = True
            if dirty:
                _save_state(group, state)

        # For one-time rules, a successful execution should invalidate the rule itself
        # (persisted in group.yaml), not only runtime state. This prevents re-sending
        # after blueprint export/import where state files are not carried over.
        one_time_completed = [
            rid
            for rid, (sent_any, _last_error, trigger_kind, _slot_key) in results.items()
            if sent_any and trigger_kind == "at"
        ]
        if one_time_completed:
            disable_errors: Dict[str, str] = {}
            for rid in one_time_completed:
                ok, err = self._daemon_automation_call(
                    op="group_automation_manage",
                    args={
                        "group_id": group.group_id,
                        "by": "user",
                        "actions": [
                            {
                                "type": "set_rule_enabled",
                                "rule_id": rid,
                                "enabled": False,
                            }
                        ],
                    },
                )
                if not ok:
                    disable_errors[rid] = str(err or "failed to disable one-time rule")[:500]

            if disable_errors:
                with self._lock:
                    state = _load_state(group)
                    dirty = False
                    for rid, err in disable_errors.items():
                        st = _rule_state(state, rid)
                        st["last_error_at"] = now_iso
                        st["last_error"] = f"auto-disable failed: {err}"[:500]
                        dirty = True
                    if dirty:
                        _save_state(group, state)

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
