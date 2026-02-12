"""Message delivery module for CCCC daemon.

This module handles:
1. Lazy Preamble: System prompt is delivered with the first message, not at actor startup
2. Message Throttling: Batches messages within a time window to prevent message bombing
3. MCP Reminders: Periodically reminds actors to use MCP tools for messaging
4. Delivery Formatting: Renders messages in IM-style format for PTY injection
5. State-aware Delivery: Respects group state (active/idle/paused)

Key design decisions:
- delivery_min_interval_seconds: Minimum interval between deliveries (default 0s)
- Messages within the window are batched and delivered together
- Reminders are injected every N chat messages (per actor) to reduce "stdout-only" replies

Group State Behavior:
- active: All messages delivered normally
- idle: chat.message delivered (auto-transitions to active), system.notify blocked
- paused: All PTY delivery blocked (messages accumulate in inbox only)
"""
from __future__ import annotations

import json
import logging
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

logger = logging.getLogger("cccc.delivery")

from ..kernel.actors import find_actor, list_actors
from ..kernel.group import Group, get_group_state, set_group_state
from ..kernel.inbox import is_message_for_actor, set_cursor
from ..kernel.system_prompt import render_system_prompt
from ..paths import ensure_home
from ..runners import pty as pty_runner
from ..runners import headless as headless_runner
from ..util.fs import atomic_write_text, read_json
from ..util.time import parse_utc_iso, utc_now_iso
from ..util.conv import coerce_bool


# ============================================================================
# Configuration
# ============================================================================

DEFAULT_DELIVERY_MIN_INTERVAL_SECONDS = 0  # Minimum interval between deliveries
DEFAULT_DELIVERY_RETRY_INTERVAL_SECONDS = 5  # Retry interval when delivery cannot be completed
PTY_SUBMIT_DELAY_SECONDS = 1.5  # Empirically reliable for CLI readline timing (payload then delayed submit)
PREAMBLE_TO_MESSAGE_DELAY_SECONDS = 2.0  # Wait for preamble submit + a small buffer
PTY_STARTUP_MAX_WAIT_SECONDS = 10.0  # Max wait for a fresh runtime to become ready before first PTY injection
PTY_STARTUP_READY_GRACE_SECONDS = 1.0  # Extra safety delay after readiness is detected (user request)
### NOTE
# Delivery is intentionally daemon-driven (single-writer). If a service needs to notify actors, it
# should call the daemon IPC and retry on transient failures rather than writing directly to ledgers.


def _get_delivery_config(group: Group) -> Dict[str, Any]:
    """Get delivery configuration from group.yaml."""
    delivery = group.doc.get("delivery")
    if not isinstance(delivery, dict):
        delivery = {}
    try:
        min_interval = int(delivery.get("min_interval_seconds", DEFAULT_DELIVERY_MIN_INTERVAL_SECONDS))
    except Exception:
        min_interval = int(DEFAULT_DELIVERY_MIN_INTERVAL_SECONDS)
    min_interval = max(0, min_interval)
    return {
        "min_interval_seconds": min_interval,
        "auto_mark_on_delivery": coerce_bool(delivery.get("auto_mark_on_delivery"), default=False),
    }


def _get_auto_mark_on_delivery(group: Group) -> bool:
    """Get auto_mark_on_delivery setting from group.yaml delivery config."""
    delivery = group.doc.get("delivery")
    if not isinstance(delivery, dict):
        return False
    return coerce_bool(delivery.get("auto_mark_on_delivery"), default=False)


# ============================================================================
# State-aware Delivery Helpers
# ============================================================================


def should_deliver_message(group: Group, kind: str) -> bool:
    """Check if a message should be delivered based on group state.
    
    Args:
        group: The group to check
        kind: Message kind ("chat.message" or "system.notify")
    
    Returns:
        True if the message should be delivered to PTY, False if it should only go to inbox
    
    State behavior:
        - active: All messages delivered
        - idle: chat.message delivered, system.notify blocked (no auto state transition here)
        - paused: All messages blocked (inbox only)
    """
    state = get_group_state(group)
    
    if state == "paused":
        # Paused: block all PTY delivery
        return False
    
    if state == "idle":
        # Idle: only chat.message allowed (system.notify blocked).
        #
        # IMPORTANT: Do NOT auto-transition to active here. Waking an idle group is
        # handled at message-ingest time (e.g. user sends a new message), otherwise
        # agent-to-agent chatter or delayed/throttled deliveries can accidentally
        # flip idle -> active and re-enable automation.
        return kind == "chat.message"
    
    # Active: all messages delivered
    return True


# ============================================================================
# Lazy Preamble (System Prompt)
# ============================================================================


def _preamble_state_path(group: Group) -> Path:
    """Return the on-disk preamble state file path."""
    return group.path / "state" / "preamble_sent.json"


def _load_preamble_sent(group: Group) -> Dict[str, str]:
    """Load preamble delivery state.

    State is scoped to the current PTY session:
      {actor_id: session_key}
    where session_key changes on every actor start/restart.
    """
    p = _preamble_state_path(group)
    try:
        data = read_json(p)
        if isinstance(data, dict):
            out: Dict[str, str] = {}
            for k, v in data.items():
                aid = str(k)
                if isinstance(v, str):
                    out[aid] = v
            return out
    except Exception:
        pass
    return {}


def _save_preamble_sent(group: Group, state: Dict[str, str]) -> None:
    """Persist preamble delivery state."""
    p = _preamble_state_path(group)
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        atomic_write_text(p, json.dumps(state, ensure_ascii=False, indent=2))
    except Exception:
        pass


def _current_preamble_session_key(group: Group, actor_id: str) -> Optional[str]:
    """Return the current PTY session key for this actor (None if not running)."""
    gid = str(group.group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return None
    try:
        return pty_runner.SUPERVISOR.session_key(group_id=gid, actor_id=aid)
    except Exception:
        return None


def is_preamble_sent(group: Group, actor_id: str) -> bool:
    """Return True if the actor has received the system prompt in the current PTY session."""
    aid = str(actor_id or "").strip()
    if not aid:
        return False
    sk = _current_preamble_session_key(group, aid)
    if not sk:
        return False
    state = _load_preamble_sent(group)
    return state.get(aid) == sk


def mark_preamble_sent(group: Group, actor_id: str) -> None:
    """Mark the system prompt as delivered for the actor's current PTY session."""
    aid = str(actor_id or "").strip()
    if not aid:
        return
    sk = _current_preamble_session_key(group, aid)
    if not sk:
        # No running session; do not record a stale "sent" state.
        return
    state = _load_preamble_sent(group)
    state[aid] = sk
    _save_preamble_sent(group, state)


def clear_preamble_sent(group: Group, actor_id: Optional[str] = None) -> None:
    """Clear preamble delivery state (e.g., on actor restart).

    If actor_id is None, clears state for all actors in the group.
    """
    if actor_id is None:
        _save_preamble_sent(group, {})
    else:
        state = _load_preamble_sent(group)
        state.pop(actor_id, None)
        _save_preamble_sent(group, state)


# ============================================================================
# Delivery Throttle (batching/throttling)
# ============================================================================


@dataclass
class PendingMessage:
    """A message pending delivery."""
    event_id: str
    by: str
    to: List[str]
    text: str
    reply_to: Optional[str] = None
    quote_text: Optional[str] = None
    ts: str = ""
    kind: str = "chat.message"  # chat.message or system.notify
    notify_kind: str = ""  # For system.notify: nudge, keepalive, etc.
    notify_title: str = ""
    notify_message: str = ""


@dataclass
class ActorDeliveryState:
    """Delivery state for a single actor."""
    last_delivery_at: Optional[datetime] = None
    last_attempt_at: Optional[datetime] = None
    pending_messages: List[PendingMessage] = field(default_factory=list)
    delivered_chat_count: int = 0  # Count of delivered chat.message (per actor, in-memory)


class DeliveryThrottle:
    """Manages message delivery throttling for all groups/actors.
    
    Key behavior:
    - Messages are queued and delivered in batches
    - Minimum interval between deliveries is configurable (default 0s)
    - A periodic reminder can be injected by the delivery layer
    """
    
    def __init__(self) -> None:
        self._lock = threading.Lock()
        # State: {group_id: {actor_id: ActorDeliveryState}}
        self._states: Dict[str, Dict[str, ActorDeliveryState]] = {}
    
    def _get_state(self, group_id: str, actor_id: str) -> ActorDeliveryState:
        """Get or create delivery state for an actor."""
        if group_id not in self._states:
            self._states[group_id] = {}
        if actor_id not in self._states[group_id]:
            self._states[group_id][actor_id] = ActorDeliveryState()
        return self._states[group_id][actor_id]
    
    def queue_message(
        self,
        group_id: str,
        actor_id: str,
        *,
        event_id: str,
        by: str,
        to: List[str],
        text: str,
        reply_to: Optional[str] = None,
        quote_text: Optional[str] = None,
        ts: str = "",
        kind: str = "chat.message",
        notify_kind: str = "",
        notify_title: str = "",
        notify_message: str = "",
    ) -> None:
        """Queue a message for delivery."""
        with self._lock:
            state = self._get_state(group_id, actor_id)
            logger.debug(f"[THROTTLE] queue_message: {group_id}/{actor_id} event={event_id} text={text[:50]!r} pending_before={len(state.pending_messages)}")
            state.pending_messages.append(PendingMessage(
                event_id=event_id,
                by=by,
                to=to,
                text=text,
                reply_to=reply_to,
                quote_text=quote_text,
                ts=ts or utc_now_iso(),
                kind=kind,
                notify_kind=notify_kind,
                notify_title=notify_title,
                notify_message=notify_message,
            ))
    
    def should_deliver(self, group_id: str, actor_id: str, min_interval_seconds: int) -> bool:
        """Check if we should deliver messages now."""
        with self._lock:
            state = self._get_state(group_id, actor_id)
            if not state.pending_messages:
                return False
            now = datetime.now(timezone.utc)
            # If we've never successfully delivered, allow attempts with a short retry backoff.
            if state.last_delivery_at is None:
                if state.last_attempt_at is None:
                    return True
                elapsed_attempt = (now - state.last_attempt_at).total_seconds()
                return elapsed_attempt >= DEFAULT_DELIVERY_RETRY_INTERVAL_SECONDS

            elapsed_delivery = (now - state.last_delivery_at).total_seconds()
            if elapsed_delivery < min_interval_seconds:
                return False

            # Past min-interval since last successful delivery; still avoid tight retry loops.
            if state.last_attempt_at is None:
                return True
            elapsed_attempt = (now - state.last_attempt_at).total_seconds()
            return elapsed_attempt >= DEFAULT_DELIVERY_RETRY_INTERVAL_SECONDS
    
    def take_pending(self, group_id: str, actor_id: str) -> List[PendingMessage]:
        """Take and clear pending messages for an actor (marks a delivery attempt)."""
        with self._lock:
            state = self._get_state(group_id, actor_id)
            messages = state.pending_messages
            logger.debug(f"[THROTTLE] take_pending: {group_id}/{actor_id} count={len(messages)}")
            state.pending_messages = []
            state.last_attempt_at = datetime.now(timezone.utc)
            return messages

    def requeue_front(self, group_id: str, actor_id: str, messages: List[PendingMessage]) -> None:
        """Requeue messages at the front to preserve ordering across retries."""
        if not messages:
            return
        with self._lock:
            state = self._get_state(group_id, actor_id)
            logger.debug(f"[THROTTLE] requeue_front: {group_id}/{actor_id} requeue={len(messages)} existing={len(state.pending_messages)}")
            state.pending_messages = list(messages) + state.pending_messages

    def mark_delivered(self, group_id: str, actor_id: str) -> None:
        """Mark a successful delivery timestamp for an actor."""
        with self._lock:
            state = self._get_state(group_id, actor_id)
            state.last_delivery_at = datetime.now(timezone.utc)
            # A successful delivery should not trigger retry backoff gating.
            state.last_attempt_at = None

    def get_delivered_chat_count(self, group_id: str, actor_id: str) -> int:
        """Get delivered chat.message count for an actor (in-memory)."""
        with self._lock:
            state = self._get_state(group_id, actor_id)
            return int(state.delivered_chat_count or 0)

    def add_delivered_chat_count(self, group_id: str, actor_id: str, delta: int) -> None:
        """Add delivered chat.message count for an actor (in-memory)."""
        d = int(delta or 0)
        if d <= 0:
            return
        with self._lock:
            state = self._get_state(group_id, actor_id)
            state.delivered_chat_count = int(state.delivered_chat_count or 0) + d
    
    def has_pending(self, group_id: str, actor_id: str) -> bool:
        """Check if actor has pending messages."""
        with self._lock:
            state = self._get_state(group_id, actor_id)
            return len(state.pending_messages) > 0

    def clear_actor(self, group_id: str, actor_id: str) -> None:
        """Clear all state for an actor (e.g., on restart)."""
        with self._lock:
            pending_count = 0
            if group_id in self._states and actor_id in self._states[group_id]:
                pending_count = len(self._states[group_id][actor_id].pending_messages)
                del self._states[group_id][actor_id]
            logger.debug(f"[THROTTLE] clear_actor: {group_id}/{actor_id} cleared_pending={pending_count}")

    def reset_actor(self, group_id: str, actor_id: str, *, keep_pending: bool = True) -> None:
        """Reset delivery metadata for an actor without dropping queued messages.

        This is important for correctness during daemon/group/actor restarts:
        messages can be queued while the actor is (re)starting. If we clear all
        state we can accidentally drop those queued messages, leaving them only
        in the inbox/ledger and never delivering to the PTY.
        """
        with self._lock:
            state = self._get_state(group_id, actor_id)
            pending = list(state.pending_messages) if keep_pending else []
            state.last_delivery_at = None
            state.last_attempt_at = None
            state.delivered_chat_count = 0
            state.pending_messages = pending

    def clear_pending_system_notifies(self, group_id: str, *, notify_kinds: Optional[set[str]] = None) -> int:
        """Remove pending system.notify messages for a group.

        Used to prevent "catch-up" notification bursts when resuming from
        idle/paused.
        """
        gid = str(group_id or "").strip()
        if not gid:
            return 0
        with self._lock:
            actors = self._states.get(gid)
            if not isinstance(actors, dict) or not actors:
                return 0
            removed = 0
            for st in actors.values():
                before = len(st.pending_messages)
                if not before:
                    continue
                kept: list[PendingMessage] = []
                for msg in st.pending_messages:
                    if msg.kind != "system.notify":
                        kept.append(msg)
                        continue
                    if notify_kinds is not None and msg.notify_kind not in notify_kinds:
                        kept.append(msg)
                        continue
                st.pending_messages = kept
                removed += before - len(kept)
            return removed

    def debug_summary(self, group_id: str) -> Dict[str, Any]:
        """Return a bounded, read-only summary for developer-mode debugging."""
        gid = str(group_id or "").strip()
        if not gid:
            return {}
        with self._lock:
            actors = self._states.get(gid) or {}
            out: Dict[str, Any] = {"group_id": gid, "actors": {}, "pending_total": 0}
            for aid, st in actors.items():
                if not isinstance(st, ActorDeliveryState):
                    continue
                pending = list(st.pending_messages) if st.pending_messages else []
                pending_total = len(pending)
                out["pending_total"] = int(out.get("pending_total") or 0) + pending_total
                last_delivery = st.last_delivery_at.isoformat().replace("+00:00", "Z") if st.last_delivery_at else None
                last_attempt = st.last_attempt_at.isoformat().replace("+00:00", "Z") if st.last_attempt_at else None
                # Keep only a small preview of pending kinds (no message bodies).
                kinds = []
                for m in pending[:20]:
                    try:
                        kinds.append(str(m.kind or ""))
                    except Exception:
                        continue
                out["actors"][str(aid)] = {
                    "pending": pending_total,
                    "pending_kinds_preview": kinds,
                    "last_delivery_at": last_delivery,
                    "last_attempt_at": last_attempt,
                    "delivered_chat_count": int(st.delivered_chat_count or 0),
                }
            return out


# Global throttle instance
THROTTLE = DeliveryThrottle()


# ============================================================================
# Message Rendering
# ============================================================================

REMINDER_EVERY_N_MESSAGES = 1
MCP_REMINDER_LINE = (
    "[cccc] If you respond: use MCP (cccc_message_send / cccc_message_reply). "
    "Terminal output isn't delivered. Help: cccc_help."
)


def render_single_message(msg: PendingMessage) -> str:
    """Render a single message for PTY delivery."""
    if msg.kind == "system.notify":
        # System notification format
        return f"[cccc] SYSTEM ({msg.notify_kind}): {msg.notify_title}\n{msg.notify_message}".strip()
    
    # Chat message format
    who = str(msg.by or "user").strip() or "user"
    targets = ", ".join([str(x).strip() for x in (msg.to or []) if str(x).strip()]) or "@all"
    body = (msg.text or "").rstrip("\n")

    # Build header
    head = f"[cccc] {who} → {targets}"
    if msg.reply_to:
        head += f" (reply:{msg.reply_to[:8]})"

    # Add quote if present
    if msg.quote_text:
        quote_preview = msg.quote_text[:80].replace("\n", " ")
        if len(msg.quote_text) > 80:
            quote_preview += "..."
        head += f'\n> "{quote_preview}"'

    return f"{head}:\n{body}" if "\n" in body else f"{head}: {body}"


def render_batched_messages(messages: List[PendingMessage], *, reminder_after_index: Optional[int] = None) -> str:
    """Render multiple messages as a batch for PTY delivery."""
    if not messages:
        return ""

    blocks: List[str] = []
    if len(messages) > 1:
        blocks.append(f"[cccc] {len(messages)} new messages:")

    for i, msg in enumerate(messages, 1):
        blocks.append(render_single_message(msg))

    if reminder_after_index is not None:
        blocks.append(MCP_REMINDER_LINE)

    return "\n\n".join([b for b in blocks if b]).rstrip()


# ============================================================================
# Legacy render function (for backward compatibility)
# ============================================================================


def render_delivery_text(
    *,
    by: str,
    to: list[str],
    text: str,
    reply_to: Optional[str] = None,
    quote_text: Optional[str] = None,
) -> str:
    """Render a single message for PTY delivery (legacy interface)."""
    msg = PendingMessage(
        event_id="",
        by=by,
        to=to,
        text=text,
        reply_to=reply_to,
        quote_text=quote_text,
    )
    return render_single_message(msg)


# ============================================================================
# PTY Submission
# ============================================================================


def pty_submit_text(
    group: Group,
    *,
    actor_id: str,
    text: str,
    file_fallback: bool = False,
    wait_for_submit: bool = False,
) -> bool:
    """Send message text to a PTY session.

    Strategy:
    1) Write the payload (prefer bracketed-paste wrapper when supported).
    2) Send a delayed "submit" (Enter/Newline) to let the CLI apply input reliably.
    """
    gid = str(group.group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        logger.warning(f"[pty_submit_text] Missing gid={gid} or aid={aid}")
        return False
    if not pty_runner.SUPERVISOR.actor_running(gid, aid):
        logger.warning(f"[pty_submit_text] Actor not running: {gid}/{aid}")
        return False

    raw = (text or "").rstrip("\n")
    if not raw:
        logger.warning(f"[pty_submit_text] Empty text for {gid}/{aid}")
        return False

    multiline = ("\n" in raw) or ("\r" in raw)
    
    # Determine submit mode for this actor.
    submit = b"\r"
    actor = find_actor(group, aid)
    mode = str(actor.get("submit") if isinstance(actor, dict) else "") or "enter"
    if mode == "none":
        submit = b""
    elif mode == "newline":
        submit = b"\n"

    payload = raw.encode("utf-8", errors="replace")
    
    logger.debug(f"[pty_submit_text] Sending to {gid}/{aid}: multiline={multiline}, mode={mode}, len={len(payload)}")
    
    # NOTE: bracketed paste is negotiated as terminal output -> emulator; here we write to stdin.
    # Do NOT write \x1b[?2004h/\x1b[?2004l as input (it becomes garbage input and can break TUIs).
    bracketed = False
    try:
        bracketed = bool(pty_runner.SUPERVISOR.bracketed_paste_enabled(group_id=gid, actor_id=aid))
    except Exception:
        bracketed = False

    # Prefer sending the full message as a single "paste" payload (may include newlines).
    # Only wrap when the target app has enabled bracketed paste; otherwise ESC codes become raw input.
    if bracketed:
        text_payload = b"\x1b[200~" + payload + b"\x1b[201~"
    else:
        text_payload = payload
    
    # Step 1: write payload
    ok = bool(pty_runner.SUPERVISOR.write_input(group_id=gid, actor_id=aid, data=text_payload))
    if not ok:
        logger.warning(f"[pty_submit_text] Failed to write payload to {gid}/{aid}")
        return False
    logger.debug(f"[pty_submit_text] Sent text payload, scheduling delayed submit")
    
    # Step 2: send submit (Enter/Newline) after a small delay for CLI timing
    if submit:
        if wait_for_submit:
            time.sleep(PTY_SUBMIT_DELAY_SECONDS)
            if not pty_runner.SUPERVISOR.actor_running(gid, aid):
                logger.warning(f"[pty_submit_text] Actor no longer running for delayed submit: {gid}/{aid}")
                return False
            ok_submit = bool(pty_runner.SUPERVISOR.write_input(group_id=gid, actor_id=aid, data=submit))
            if not ok_submit:
                logger.warning(f"[pty_submit_text] Delayed submit failed to write to {gid}/{aid}")
                return False
            logger.debug(f"[pty_submit_text] Delayed submit sent to {gid}/{aid}")
        else:
            def delayed_submit():
                time.sleep(PTY_SUBMIT_DELAY_SECONDS)  # Empirically reliable delay
                if pty_runner.SUPERVISOR.actor_running(gid, aid):
                    ok_submit = bool(pty_runner.SUPERVISOR.write_input(group_id=gid, actor_id=aid, data=submit))
                    if ok_submit:
                        logger.debug(f"[pty_submit_text] Delayed submit sent to {gid}/{aid}")
                    else:
                        logger.warning(f"[pty_submit_text] Delayed submit failed to write to {gid}/{aid}")
                else:
                    logger.warning(f"[pty_submit_text] Actor no longer running for delayed submit: {gid}/{aid}")

            submit_thread = threading.Thread(target=delayed_submit, daemon=True)
            submit_thread.start()
    
    return True


# ============================================================================
# High-level Delivery Functions
# ============================================================================


def deliver_message_with_preamble(
    group: Group,
    *,
    actor_id: str,
    message_text: str,
    by: str,
) -> bool:
    """Deliver a message to PTY, prepending the system prompt on first delivery.

    This is the core of the lazy preamble mechanism:
    - If the actor hasn't received the system prompt in this PTY session, deliver it first.
    - Then deliver the user message.

    Note: this function bypasses throttling and exists for backward compatibility.
    New code should queue messages and use flush_pending_messages().
    """
    aid = str(actor_id or "").strip()
    if not aid:
        return False
    
    actor = find_actor(group, aid)
    if not isinstance(actor, dict):
        return False
    
    # Deliver system prompt first (lazy preamble) when needed.
    if not is_preamble_sent(group, aid):
        try:
            prompt = render_system_prompt(group=group, actor=actor)
            if prompt and prompt.strip():
                if pty_submit_text(group, actor_id=aid, text=prompt, file_fallback=True, wait_for_submit=True):
                    mark_preamble_sent(group, aid)
        except Exception:
            pass
    
    # Deliver user message
    delivered_before = THROTTLE.get_delivered_chat_count(group.group_id, aid)
    out = (message_text or "").rstrip("\n")
    if out and (delivered_before + 1) % REMINDER_EVERY_N_MESSAGES == 0:
        out = out + "\n\n" + MCP_REMINDER_LINE
    result = pty_submit_text(group, actor_id=aid, text=out)
    if result:
        THROTTLE.add_delivered_chat_count(group.group_id, aid, 1)
    return result


def queue_chat_message(
    group: Group,
    *,
    actor_id: str,
    event_id: str,
    by: str,
    to: List[str],
    text: str,
    reply_to: Optional[str] = None,
    quote_text: Optional[str] = None,
    ts: str = "",
) -> None:
    """Queue a chat message for throttled delivery."""
    THROTTLE.queue_message(
        group.group_id,
        actor_id,
        event_id=event_id,
        by=by,
        to=to,
        text=text,
        reply_to=reply_to,
        quote_text=quote_text,
        ts=ts,
        kind="chat.message",
    )


def queue_system_notify(
    group: Group,
    *,
    actor_id: str,
    event_id: str,
    notify_kind: str,
    title: str,
    message: str,
    ts: str = "",
) -> None:
    """Queue a system notification for throttled delivery."""
    THROTTLE.queue_message(
        group.group_id,
        actor_id,
        event_id=event_id,
        by="system",
        to=[actor_id],
        text="",
        ts=ts,
        kind="system.notify",
        notify_kind=notify_kind,
        notify_title=title,
        notify_message=message,
    )


def flush_pending_messages(group: Group, *, actor_id: str) -> bool:
    """Flush pending messages for an actor if ready.

    Respects group state:
    - active: All messages delivered
    - idle: chat.message delivered (auto-transitions to active), system.notify blocked
    - paused: All messages blocked (stay in queue for later)

    Returns True if messages were delivered.
    """
    gid = group.group_id
    aid = actor_id

    config = _get_delivery_config(group)
    min_interval = config["min_interval_seconds"]

    if not THROTTLE.should_deliver(gid, aid, min_interval):
        return False

    # Startup gate: when a PTY actor was just started/restarted, its CLI may still be initializing and
    # can drop early stdin/TTY input. Hold the first delivery until we see some output (then wait an
    # extra grace period) or until the max startup wait elapses.
    #
    # Additionally, for the very first delivery (system prompt preamble), prefer waiting until the
    # app enables bracketed paste mode. The preamble is multi-line; sending it before bracketed paste
    # is enabled can lead to partial/ignored input in some TUIs.
    try:
        started_at, first_output_at = pty_runner.SUPERVISOR.startup_times(group_id=gid, actor_id=aid)
        if started_at is not None:
            now = time.monotonic()
            if first_output_at is not None:
                if (now - float(first_output_at)) < PTY_STARTUP_READY_GRACE_SECONDS:
                    return False
            else:
                if (now - float(started_at)) < PTY_STARTUP_MAX_WAIT_SECONDS:
                    return False
            # Wait for bracketed paste mode for the preamble (best-effort; fall back after max wait).
            if not is_preamble_sent(group, aid):
                enabled, changed_at = pty_runner.SUPERVISOR.bracketed_paste_status(group_id=gid, actor_id=aid)
                if not enabled and (now - float(started_at)) < PTY_STARTUP_MAX_WAIT_SECONDS:
                    return False
                if enabled and changed_at is not None and (now - float(changed_at)) < PTY_STARTUP_READY_GRACE_SECONDS:
                    return False
    except Exception:
        pass

    messages = THROTTLE.take_pending(gid, aid)
    if not messages:
        return False

    # Filter messages based on group state
    deliverable: List[PendingMessage] = []
    requeue: List[PendingMessage] = []

    for msg in messages:
        if should_deliver_message(group, msg.kind):
            deliverable.append(msg)
        else:
            # Message blocked by state - requeue for later
            requeue.append(msg)

    if not deliverable:
        # Nothing is deliverable in the current group state; keep everything queued.
        THROTTLE.requeue_front(gid, aid, messages)
        return False
    
    actor = find_actor(group, aid)
    if not isinstance(actor, dict):
        THROTTLE.requeue_front(gid, aid, messages)
        return False

    chat_total = sum(1 for m in deliverable if m.kind == "chat.message")
    reminder_after_index: Optional[int] = None
    if chat_total > 0:
        delivered_before = THROTTLE.get_delivered_chat_count(gid, aid)
        delivered_after = delivered_before + chat_total
        # Remind every N delivered chat messages per actor (count is in-memory).
        if (delivered_after // REMINDER_EVERY_N_MESSAGES) > (delivered_before // REMINDER_EVERY_N_MESSAGES):
            reminder_after_index = len(deliverable)
    
    # Check if preamble needs to be sent - send SEPARATELY before message
    # This is critical: sending preamble+message together as a large payload
    # causes Claude Code to miss the first message (readline buffer issue).
    preamble_already_sent = is_preamble_sent(group, aid)
    preamble_just_sent = False
    if not preamble_already_sent:
        try:
            prompt = render_system_prompt(group=group, actor=actor)
            if prompt and prompt.strip():
                preamble_ok = pty_submit_text(group, actor_id=aid, text=prompt.strip(), wait_for_submit=True)
                if preamble_ok:
                    mark_preamble_sent(group, aid)
                    preamble_just_sent = True
                    logger.debug(f"[flush] {gid}/{aid} preamble sent, will send message after delay")
                else:
                    # Preamble failed, requeue everything
                    THROTTLE.requeue_front(gid, aid, messages)
                    return False
        except Exception:
            THROTTLE.requeue_front(gid, aid, messages)
            return False

    # Render batched messages
    message_text = render_batched_messages(deliverable, reminder_after_index=reminder_after_index)

    # If preamble was just sent, schedule message delivery after delay (non-blocking)
    # pty_submit_text has 1.5s delayed submit, we need to wait for that + processing time
    if preamble_just_sent and message_text:
        logger.debug(f"[flush] {gid}/{aid} preamble sent, scheduling delayed message delivery")

        def delayed_message_send():
            time.sleep(PREAMBLE_TO_MESSAGE_DELAY_SECONDS)  # Wait for preamble submit + CLI processing buffer
            if not pty_runner.SUPERVISOR.actor_running(gid, aid):
                # Actor stopped before we could deliver; keep messages queued for next restart.
                logger.warning(f"[flush] {gid}/{aid} actor no longer running, re-queueing delayed message(s)")
                THROTTLE.requeue_front(gid, aid, messages)
                return
            logger.debug(f"[flush] {gid}/{aid} sending delayed message now")
            ok = bool(pty_submit_text(group, actor_id=aid, text=message_text))
            if ok:
                THROTTLE.add_delivered_chat_count(gid, aid, chat_total)
                THROTTLE.mark_delivered(gid, aid)
                # Auto-mark as read if enabled
                if _get_auto_mark_on_delivery(group) and deliverable:
                    last_msg = deliverable[-1]
                    try:
                        set_cursor(group, aid, event_id=last_msg.event_id, ts=last_msg.ts)
                        logger.debug(f"[flush] {gid}/{aid} auto-marked {len(deliverable)} messages as read")
                    except Exception as e:
                        logger.warning(f"[flush] {gid}/{aid} auto-mark failed: {e}")
                if requeue:
                    THROTTLE.requeue_front(gid, aid, requeue)
            else:
                # Failed, requeue for retry
                THROTTLE.requeue_front(gid, aid, messages)

        send_thread = threading.Thread(target=delayed_message_send, daemon=True)
        send_thread.start()
        return True  # Preamble delivered, message scheduled

    # Send message (no preamble delay needed)
    delivered = False
    if message_text:
        delivered = bool(pty_submit_text(group, actor_id=aid, text=message_text))
        if delivered:
            THROTTLE.add_delivered_chat_count(gid, aid, chat_total)
            THROTTLE.mark_delivered(gid, aid)
            # Auto-mark as read if enabled
            if _get_auto_mark_on_delivery(group) and deliverable:
                last_msg = deliverable[-1]
                try:
                    set_cursor(group, aid, event_id=last_msg.event_id, ts=last_msg.ts)
                    logger.debug(f"[flush] {gid}/{aid} auto-marked {len(deliverable)} messages as read")
                except Exception as e:
                    logger.warning(f"[flush] {gid}/{aid} auto-mark failed: {e}")
            # Keep blocked messages queued for later.
            if requeue:
                THROTTLE.requeue_front(gid, aid, requeue)
        else:
            # Delivery failed: keep everything queued for retry.
            THROTTLE.requeue_front(gid, aid, messages)
    
    return delivered


def tick_delivery(group: Group) -> None:
    """Called periodically to flush pending messages for all actors."""
    pty_supported = bool(getattr(pty_runner, "PTY_SUPPORTED", True))
    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        aid = str(actor.get("id") or "").strip()
        if not aid:
            continue
        # Only for PTY runners
        runner_kind = str(actor.get("runner") or "pty").strip()
        if runner_kind != "pty" or not pty_supported:
            continue
        # Check if actor process is actually running
        if not pty_runner.SUPERVISOR.actor_running(group.group_id, aid):
            continue

        try:
            flush_pending_messages(group, actor_id=aid)
        except Exception:
            pass


def inject_system_prompt(group: Group, *, actor: Dict[str, Any]) -> None:
    """Inject system prompt into actor's PTY (for manual refresh)."""
    aid = str(actor.get("id") or "").strip()
    if not aid:
        return
    prompt = render_system_prompt(group=group, actor=actor)
    pty_submit_text(group, actor_id=aid, text=prompt, file_fallback=True, wait_for_submit=True)


def get_headless_targets_for_message(
    group: Group,
    *,
    event: Dict[str, Any],
    by: str,
) -> List[str]:
    """Return headless actor_ids that should be notified for a message.

    This function only computes targets; the daemon server performs any writes.
    """
    targets: List[str] = []
    
    pty_supported = bool(getattr(pty_runner, "PTY_SUPPORTED", True))
    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        aid = str(actor.get("id") or "").strip()
        if not aid or aid == "user" or aid == by:
            continue
        
        # Headless runner only (or PTY fallback when PTY is unsupported)
        runner_kind = str(actor.get("runner") or "pty").strip()
        if runner_kind != "headless" and not (runner_kind == "pty" and not pty_supported):
            continue
        
        # Actor must be running
        if not headless_runner.SUPERVISOR.actor_running(group.group_id, aid):
            continue
        
        # Check delivery/visibility rules
        if not is_message_for_actor(group, actor_id=aid, event=event):
            continue
        
        targets.append(aid)
    
    return targets
