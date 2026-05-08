"""Browser-delivery adapter for website-hosted model actors.

This module owns the daemon-side protocol boundary. ChatGPT delivery uses the
shared projected browser session so browser writes are serialized through one
daemon-owned command queue.
"""

from __future__ import annotations

import json
import logging
import os
import threading
import hashlib
from typing import Any, Dict, List, Optional

from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.inbox import unread_messages
from ...kernel.ledger import append_event
from ...kernel.system_prompt import render_system_prompt
from ...kernel.web_model_connectors import list_web_model_connectors
from ...util.time import parse_utc_iso, utc_now_iso
from ...ports.web_model_browser_sidecar import (
    CHATGPT_URL,
    _conversation_url_from_tab,
    read_chatgpt_browser_state,
    record_chatgpt_browser_state,
    resolve_pending_chatgpt_conversation,
)
from ..messaging.actor_turn_rendering import render_actor_event_batch_for_delivery
from ..messaging.delivery import MCP_REMINDER_LINE
from ..runner_state_ops import read_headless_state, update_headless_state
from .web_model_runtime_ops import commit_web_model_delivered_turn

_LOG = logging.getLogger("cccc.daemon.web_model.browser_delivery")
_IN_FLIGHT_LOCK = threading.Lock()
_IN_FLIGHT: set[tuple[str, str]] = set()

_MODE_ENV_KEYS = (
    "CCCC_WEB_MODEL_DELIVERY_MODE",
    "CCCC_WEB_MODEL_DELIVERY",
)
_PROVIDER_ENV_KEYS = (
    "CCCC_WEB_MODEL_PROVIDER",
    "CCCC_WEB_MODEL_BROWSER_PROVIDER",
)
_BROWSER_PROVIDERS = {"chatgpt_web", "browser_web_model", "chatgpt_browser"}
_PULL_MODES = {"", "pull", "native", "remote_mcp", "off", "disabled", "none"}
_EXPLICIT_PULL_MODES = _PULL_MODES - {""}
_BROWSER_MODES = {"browser", "chatgpt", "chatgpt_browser", "browser_delivery"}
_DEFAULT_TIMEOUT_SECONDS = 30.0
_PROMPT_TEXT_LIMIT = 48_000
_MAX_BROWSER_DELIVERY_EVENTS = 20
_PENDING_NEW_CHAT_RETRY_AFTER_SECONDS = 60.0
_BOOTSTRAP_SEED_VERSION = "web-model-bootstrap-normal-system-prompt-v2"


def _actor_env(actor: Dict[str, Any]) -> Dict[str, str]:
    raw = actor.get("env") if isinstance(actor, dict) else {}
    if not isinstance(raw, dict):
        return {}
    return {
        str(k): str(v)
        for k, v in raw.items()
        if isinstance(k, str) and isinstance(v, str)
    }


def _setting(actor: Dict[str, Any], keys: tuple[str, ...]) -> str:
    env = _actor_env(actor)
    for key in keys:
        value = str(env.get(key) or "").strip()
        if value:
            return value
    for key in keys:
        value = str(os.environ.get(key) or "").strip()
        if value:
            return value
    return ""


def _timeout_seconds(actor: Dict[str, Any]) -> float:
    raw = _setting(actor, ("CCCC_WEB_MODEL_BROWSER_DELIVERY_TIMEOUT_SECONDS",))
    if not raw:
        return _DEFAULT_TIMEOUT_SECONDS
    try:
        value = float(raw)
    except Exception:
        return _DEFAULT_TIMEOUT_SECONDS
    return max(5.0, min(value, 3600.0))


def _float_setting(actor: Dict[str, Any], keys: tuple[str, ...], *, default: float, minimum: float, maximum: float) -> float:
    raw = _setting(actor, keys)
    if not raw:
        return default
    try:
        value = float(raw)
    except Exception:
        return default
    return max(minimum, min(value, maximum))


def _provider_from_actor_or_connector(group_id: str, actor: Dict[str, Any]) -> str:
    actor_provider = str(actor.get("web_model_provider") or "").strip().lower()
    if actor_provider:
        return actor_provider
    env_provider = _setting(actor, _PROVIDER_ENV_KEYS).strip().lower()
    if env_provider:
        return env_provider
    actor_id = str(actor.get("id") or "").strip()
    if not group_id or not actor_id:
        return ""
    try:
        for connector in list_web_model_connectors():
            if bool(connector.get("revoked")):
                continue
            if str(connector.get("group_id") or "").strip() != group_id:
                continue
            if str(connector.get("actor_id") or "").strip() != actor_id:
                continue
            provider = str(connector.get("provider") or "").strip().lower()
            if provider:
                return provider
    except Exception:
        return ""
    return ""


def web_model_browser_delivery_enabled(group_id: str, actor: Dict[str, Any]) -> bool:
    if not isinstance(actor, dict):
        return False
    if str(actor.get("runtime") or "").strip().lower() != "web_model":
        return False
    if str(actor.get("runner") or "headless").strip().lower() != "headless":
        return False
    mode = str(actor.get("web_model_delivery_mode") or "").strip().lower()
    mode = mode or _setting(actor, _MODE_ENV_KEYS).strip().lower()
    if mode in _EXPLICIT_PULL_MODES:
        return False
    browser_requested = (
        mode in _BROWSER_MODES
        or _provider_from_actor_or_connector(group_id, actor) in _BROWSER_PROVIDERS
    )
    if not browser_requested:
        return False
    return True


def _build_web_model_bootstrap_seed(group: Any, actor: Dict[str, Any]) -> str:
    base_prompt = render_system_prompt(group=group, actor=actor).strip()
    transport_note = (
        "[CCCC] Web transport:\n"
        "- This ChatGPT conversation is the browser surface for the actor above.\n"
        "- Browser-injected messages are already delivered in chat; do not call cccc_runtime_wait_next_turn for them.\n"
        "- Use CCCC MCP tools for visible replies, handoffs, local workspace work, validation, and evidence.\n"
        "- For non-trivial local development work, default to cccc_code_exec so repo reads, patches, tests, diffs, "
        "and reports stay in one focused Codex-style loop; use direct tools only for simple one-step actions.\n"
        "- If CCCC MCP tools are not visible in the selected ChatGPT model, you do not have CCCC local access "
        "in this chat; tell the user to switch to a GPT-5.x ChatGPT session that can see the CCCC connector.\n"
        "- Text typed only in this web chat is not delivered to CCCC users or peers."
    )
    return "\n\n".join(
        [
            "[CCCC] Session bootstrap for this browser chat:",
            base_prompt,
            transport_note,
        ]
    ).strip()


def _bootstrap_seed_digest(seed_text: str) -> str:
    return hashlib.sha256(str(seed_text or "").encode("utf-8", errors="replace")).hexdigest()[:20]


def _compact_delivery_event(event: Dict[str, Any]) -> Dict[str, Any]:
    data = event.get("data")
    return {
        "id": str(event.get("id") or ""),
        "ts": str(event.get("ts") or ""),
        "kind": str(event.get("kind") or ""),
        "by": str(event.get("by") or ""),
        "scope_key": str(event.get("scope_key") or ""),
        "data": data if isinstance(data, dict) else {},
    }


def _browser_delivery_id(*, group_id: str, actor_id: str, messages: List[Dict[str, Any]]) -> str:
    payload = {
        "group_id": group_id,
        "actor_id": actor_id,
        "event_ids": [str(item.get("id") or "") for item in messages],
    }
    digest = hashlib.sha256(json.dumps(payload, ensure_ascii=False, sort_keys=True).encode("utf-8")).hexdigest()[:20]
    return f"webdelivery:{actor_id}:{digest}"


def _browser_delivery_batch(group: Any, *, actor_id: str) -> Dict[str, Any]:
    messages = unread_messages(group, actor_id=actor_id, limit=_MAX_BROWSER_DELIVERY_EVENTS, kind_filter="all")
    compact_messages = [_compact_delivery_event(event) for event in messages]
    latest = compact_messages[-1] if compact_messages else {}
    delivery_id = _browser_delivery_id(group_id=group.group_id, actor_id=actor_id, messages=compact_messages) if compact_messages else ""
    return {
        "delivery_id": delivery_id,
        "turn_id": delivery_id,
        "group_id": group.group_id,
        "actor_id": actor_id,
        "created_at": utc_now_iso(),
        "event_ids": [str(item.get("id") or "") for item in compact_messages if str(item.get("id") or "")],
        "latest_event_id": str(latest.get("id") or ""),
        "latest_ts": str(latest.get("ts") or ""),
        "messages": compact_messages,
        "coalesced_text": render_actor_event_batch_for_delivery(compact_messages, actor_id=actor_id),
        "delivery": {
            "mode": "browser_injection_cursor_on_submit",
            "cursor_committed": False,
            "max_events": _MAX_BROWSER_DELIVERY_EVENTS,
            "kind_filter": "all",
        },
    }


def build_web_model_browser_turn_prompt(
    turn: Dict[str, Any],
    *,
    bootstrap_seed_text: str = "",
) -> str:
    actor_id = str(turn.get("actor_id") or "").strip()
    delivery_id = str(turn.get("delivery_id") or turn.get("turn_id") or "").strip()
    event_ids = [
        str(item or "").strip()
        for item in (turn.get("event_ids") if isinstance(turn.get("event_ids"), list) else [])
        if str(item or "").strip()
    ]
    coalesced_text = str(turn.get("coalesced_text") or "").strip()
    if len(coalesced_text) > _PROMPT_TEXT_LIMIT:
        coalesced_text = coalesced_text[: _PROMPT_TEXT_LIMIT - 80].rstrip() + "\n\n[cccc] delivery text truncated"
    event_label = ",".join(event_ids) if event_ids else "-"
    setup_seed = str(bootstrap_seed_text or "").strip()
    setup_block = f"{setup_seed}\n\n" if setup_seed else ""
    reminder = str(MCP_REMINDER_LINE or "").strip()
    reminder_block = f"{reminder}\n\n" if reminder else ""
    return (
        f"{setup_block}"
        f"[cccc] Browser batch {delivery_id} events={event_label} actor={actor_id}\n"
        f"{reminder_block}"
        f"{coalesced_text}"
    )


def _record_delivery_submitting(
    group_id: str,
    actor_id: str,
    *,
    turn: Dict[str, Any],
    delivery_id: str,
    timeout_seconds: float,
) -> None:
    now = utc_now_iso()
    try:
        record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "last_delivery_at": now,
                "last_delivery_started_at": now,
                "last_turn_id": str(turn.get("turn_id") or ""),
                "last_event_ids": list(turn.get("event_ids") or []),
                "last_delivery_id": str(delivery_id or turn.get("delivery_id") or ""),
                "last_delivery_timeout_seconds": float(timeout_seconds or _DEFAULT_TIMEOUT_SECONDS),
                "last_delivery_status": "submitting",
                "last_submission_evidence": "",
                "last_send_selector": "",
                "last_error": "",
            },
        )
    except Exception:
        pass


def _is_transient_projected_browser_error(error: str) -> bool:
    lowered = str(error or "").lower()
    if any(
        fragment in lowered
        for fragment in (
            "target page, context or browser has been closed",
            "browser command timed out",
            "page.evaluate",
            "page.goto",
            "page.reload",
            "locator.click",
            "element is not visible",
            "composer input was found but could not be focused",
            "chatgpt prompt insertion did not stick",
            "chatgpt prompt was inserted but did not submit",
        )
    ):
        return True
    return False


def _append_delivery_event(
    *,
    group: Any,
    actor_id: str,
    turn: Dict[str, Any],
    kind: str,
    data: Dict[str, Any],
) -> Optional[Dict[str, Any]]:
    try:
        return append_event(
            group.ledger_path,
            kind=kind,
            group_id=group.group_id,
            scope_key="",
            by="system",
            data={
                "actor_id": actor_id,
                "turn_id": str(turn.get("turn_id") or "").strip(),
                "event_ids": list(turn.get("event_ids") or []),
                "latest_event_id": str(turn.get("latest_event_id") or "").strip(),
                **data,
            },
        )
    except Exception:
        return None


def _has_unread_work(group: Any, actor_id: str) -> bool:
    try:
        return bool(unread_messages(group, actor_id=actor_id, limit=1, kind_filter="all"))
    except Exception:
        return False


def _target_chat_url(group_id: str, actor_id: str, actor: Dict[str, Any]) -> str:
    explicit = _setting(actor, ("CCCC_WEB_MODEL_CHAT_URL", "CCCC_WEB_MODEL_CONVERSATION_URL", "CCCC_WEB_MODEL_TARGET_URL"))
    if explicit:
        return explicit
    try:
        state = read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        state = {}
    return str(state.get("conversation_url") or "").strip()


def _pending_new_chat_state(group_id: str, actor_id: str) -> Dict[str, Any]:
    try:
        state = read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        return {}
    if not bool(state.get("pending_new_chat_bind")):
        return {}
    if bool(state.get("pending_new_chat_submitted")):
        try:
            resolved = resolve_pending_chatgpt_conversation(group_id, actor_id)
        except Exception as exc:
            resolved = {"ok": False, "error": str(exc)}
        conversation_url = str(resolved.get("conversation_url") or "").strip() if isinstance(resolved, dict) else ""
        if conversation_url:
            return {
                "target_url": conversation_url,
                "auto_bind_new_chat": False,
                "resolved_pending_new_chat": True,
            }
        submitted_at = parse_utc_iso(str(state.get("pending_new_chat_submitted_at") or ""))
        now = parse_utc_iso(utc_now_iso())
        if submitted_at is not None and now is not None:
            age_seconds = (now - submitted_at).total_seconds()
        else:
            age_seconds = 0.0
        if age_seconds >= _PENDING_NEW_CHAT_RETRY_AFTER_SECONDS:
            record_chatgpt_browser_state(
                group_id,
                actor_id,
                {
                    "pending_new_chat_submitted": False,
                    "pending_new_chat_submitted_at": "",
                    "pending_new_chat_delivery_id": "",
                    "pending_new_chat_last_turn_id": "",
                    "pending_new_chat_last_event_ids": [],
                    "pending_new_chat_last_tab_url": "",
                    "bootstrap_seed_delivered_at": "",
                    "bootstrap_seed_version": "",
                    "bootstrap_seed_digest": "",
                    "bootstrap_seed_conversation_url": "",
                    "last_error": "conversation_url_pending_retry",
                },
            )
            return {
                "target_url": str(state.get("pending_new_chat_url") or CHATGPT_URL).strip() or CHATGPT_URL,
                "auto_bind_new_chat": True,
                "retry_pending_new_chat": True,
            }
        return {
            "status": "target_chat_binding_pending",
            "error": str((resolved or {}).get("error") or "target_chat_binding_pending") if isinstance(resolved, dict) else "target_chat_binding_pending",
            "auto_bind_new_chat": False,
            "pending_new_chat_submitted": True,
        }
    return {
        "target_url": str(state.get("pending_new_chat_url") or CHATGPT_URL).strip() or CHATGPT_URL,
        "auto_bind_new_chat": True,
    }


def _bootstrap_seed_required(group_id: str, actor_id: str, *, target_url: str = "", seed_digest: str = "") -> bool:
    try:
        state = read_chatgpt_browser_state(group_id, actor_id)
    except Exception:
        return True
    delivered_at_raw = str(state.get("bootstrap_seed_delivered_at") or "").strip()
    if not delivered_at_raw:
        return True
    try:
        headless_state = read_headless_state(group_id, actor_id)
    except Exception:
        headless_state = {}
    started_at = parse_utc_iso(str(headless_state.get("started_at") or ""))
    delivered_at = parse_utc_iso(delivered_at_raw)
    if started_at is not None and delivered_at is not None and started_at > delivered_at:
        return True
    if str(state.get("bootstrap_seed_version") or "").strip() != _BOOTSTRAP_SEED_VERSION:
        return True
    expected_url = str(target_url or "").strip()
    if expected_url and str(state.get("bootstrap_seed_conversation_url") or "").strip() != expected_url:
        return True
    expected_digest = str(seed_digest or "").strip()
    if expected_digest and str(state.get("bootstrap_seed_digest") or "").strip() != expected_digest:
        return True
    return False


def _mark_bootstrap_seed_delivered(group_id: str, actor_id: str, *, target_url: str = "", seed_digest: str = "") -> None:
    try:
        record_chatgpt_browser_state(
            group_id,
            actor_id,
            {
                "bootstrap_seed_delivered_at": utc_now_iso(),
                "bootstrap_seed_version": _BOOTSTRAP_SEED_VERSION,
                "bootstrap_seed_digest": str(seed_digest or "").strip(),
                "bootstrap_seed_conversation_url": str(target_url or "").strip(),
            },
        )
    except Exception:
        pass


def submit_next_web_model_browser_turn(group_id: str, actor_id: str, *, trigger_event_id: str = "") -> Dict[str, Any]:
    group = load_group(str(group_id or "").strip())
    if group is None:
        return {"ok": False, "error": "group_not_found"}
    actor = find_actor(group, str(actor_id or "").strip())
    if not isinstance(actor, dict):
        return {"ok": False, "error": "actor_not_found"}
    if not web_model_browser_delivery_enabled(group.group_id, actor):
        return {"ok": False, "error": "browser_delivery_disabled"}
    aid = str(actor_id or "").strip()
    target_url = _target_chat_url(group.group_id, aid, actor)
    pending_new_chat = {}
    if not target_url:
        pending_new_chat = _pending_new_chat_state(group.group_id, aid)
        target_url = str(pending_new_chat.get("target_url") or "").strip()
    auto_bind_new_chat = bool(pending_new_chat.get("auto_bind_new_chat"))
    pending_status = str(pending_new_chat.get("status") or "").strip()
    if pending_status == "target_chat_binding_pending" and not target_url:
        update_headless_state(group.group_id, aid, status="waiting", active_turn_id="", latest_event_id="")
        return {
            "ok": True,
            "status": "target_chat_binding_pending",
            "error": str(pending_new_chat.get("error") or pending_status),
            "reschedule": False,
        }
    if not target_url:
        update_headless_state(group.group_id, aid, status="waiting", active_turn_id="", latest_event_id="")
        return {"ok": False, "status": "target_chat_required", "error": "target_chat_required"}
    turn = _browser_delivery_batch(group, actor_id=aid)
    if not turn.get("event_ids"):
        update_headless_state(group.group_id, aid, status="waiting", active_turn_id="", latest_event_id="")
        return {"ok": True, "status": "idle"}

    provider = _provider_from_actor_or_connector(group.group_id, actor) or "chatgpt_web"
    candidate_seed_text = _build_web_model_bootstrap_seed(group, actor)
    seed_digest = _bootstrap_seed_digest(candidate_seed_text)
    bootstrap_seed = _bootstrap_seed_required(group.group_id, aid, target_url=target_url, seed_digest=seed_digest)
    bootstrap_seed_text = candidate_seed_text if bootstrap_seed else ""
    delivery_id = str(turn.get("delivery_id") or "")
    delivery_timeout_seconds = _timeout_seconds(actor)
    _record_delivery_submitting(
        group.group_id,
        aid,
        turn=turn,
        delivery_id=delivery_id,
        timeout_seconds=delivery_timeout_seconds,
    )
    _append_delivery_event(
        group=group,
        actor_id=aid,
        turn=turn,
        kind="web_model.browser_delivery.submitting",
        data={
            "provider": provider,
            "delivery_id": delivery_id,
            "trigger_event_id": str(trigger_event_id or "").strip(),
            "delivery_transport": "projected_session",
            "target_url": target_url,
            "auto_bind_new_chat": bool(auto_bind_new_chat),
            "timeout_seconds": float(delivery_timeout_seconds),
        },
    )
    prompt = build_web_model_browser_turn_prompt(
        turn,
        bootstrap_seed_text=bootstrap_seed_text,
    )
    browser_surface: Dict[str, Any] = {}
    try:
        from .web_model_browser_session import close_web_model_chatgpt_browser_session, submit_prompt_via_web_model_chatgpt_browser_session

        submit_kwargs = {
            "group_id": group.group_id,
            "actor_id": aid,
            "prompt": prompt,
            "target_url": target_url,
            "auto_bind_new_chat": auto_bind_new_chat,
            "delivery_id": delivery_id,
            "timeout_seconds": delivery_timeout_seconds,
            "input_timeout_seconds": _float_setting(
                actor,
                ("CCCC_WEB_MODEL_BROWSER_INPUT_TIMEOUT_SECONDS",),
                default=30.0,
                minimum=5.0,
                maximum=300.0,
            ),
            "new_chat_bind_timeout_seconds": _float_setting(
                actor,
                ("CCCC_WEB_MODEL_NEW_CHAT_BIND_TIMEOUT_SECONDS",),
                default=20.0,
                minimum=1.0,
                maximum=120.0,
            ),
        }
        try:
            delivery_result = submit_prompt_via_web_model_chatgpt_browser_session(**submit_kwargs)
        except Exception as first_exc:
            first_error = str(first_exc)
            if not _is_transient_projected_browser_error(first_error):
                raise
            try:
                close_web_model_chatgpt_browser_session(group_id=group.group_id, actor_id=aid)
            except Exception:
                pass
            _record_delivery_submitting(
                group.group_id,
                aid,
                turn=turn,
                delivery_id=delivery_id,
                timeout_seconds=delivery_timeout_seconds,
            )
            try:
                delivery_result = submit_prompt_via_web_model_chatgpt_browser_session(**submit_kwargs)
            except Exception as second_exc:
                raise RuntimeError(f"{second_exc}; retry_after_transient_error={first_error[:300]}") from second_exc
        browser_surface = delivery_result.get("browser_surface") if isinstance(delivery_result.get("browser_surface"), dict) else {}
    except Exception as exc:
        delivery_result = {"ok": False, "error": str(exc)}

    ok = bool(delivery_result.get("ok", True))
    browser_result = delivery_result.get("browser") if isinstance(delivery_result.get("browser"), dict) else {}
    delivered_conversation_url = str(browser_result.get("conversation_url") or "").strip() or _conversation_url_from_tab(
        browser_result.get("tab_url")
    )
    pending_conversation_url = bool(
        ok
        and auto_bind_new_chat
        and not delivered_conversation_url
        and (
            bool(browser_result.get("pending_conversation_url"))
            or bool(browser_result.get("submitted_without_conversation_url"))
        )
    )
    if ok and auto_bind_new_chat and not delivered_conversation_url and not pending_conversation_url:
        ok = False
        delivery_result = {
            **delivery_result,
            "ok": False,
            "error": "new ChatGPT chat did not produce a conversation URL",
        }
    if pending_conversation_url:
        pending_delivery_id = str(delivery_result.get("delivery_id") or turn.get("delivery_id") or "")
        commit = commit_web_model_delivered_turn(group, actor_id=aid, turn=turn, by=aid)
        pending_seed_state = (
            {
                "bootstrap_seed_delivered_at": utc_now_iso(),
                "bootstrap_seed_version": _BOOTSTRAP_SEED_VERSION,
                "bootstrap_seed_digest": str(seed_digest or "").strip(),
                "bootstrap_seed_conversation_url": target_url or CHATGPT_URL,
            }
            if bootstrap_seed
            else {
                "bootstrap_seed_delivered_at": "",
                "bootstrap_seed_version": "",
                "bootstrap_seed_digest": "",
                "bootstrap_seed_conversation_url": "",
            }
        )
        record_chatgpt_browser_state(
            group.group_id,
            aid,
            {
                "conversation_url": "",
                "pending_new_chat_bind": True,
                "pending_new_chat_url": target_url or CHATGPT_URL,
                "pending_new_chat_submitted": True,
                "pending_new_chat_submitted_at": utc_now_iso(),
                "pending_new_chat_delivery_id": pending_delivery_id,
                "pending_new_chat_last_turn_id": str(turn.get("turn_id") or ""),
                "pending_new_chat_last_event_ids": list(turn.get("event_ids") or []),
                "pending_new_chat_last_tab_url": str(browser_result.get("tab_url") or ""),
                **pending_seed_state,
                "last_delivery_at": utc_now_iso(),
                "last_turn_id": str(turn.get("turn_id") or ""),
                "last_event_ids": list(turn.get("event_ids") or []),
                "last_delivery_id": pending_delivery_id,
                "last_delivery_status": "pending",
                "last_submission_evidence": str(browser_result.get("submission_evidence") or ""),
                "last_send_selector": str(browser_result.get("send_selector") or ""),
                "last_error": "conversation_url_pending",
            },
        )
        update_headless_state(
            group.group_id,
            aid,
            status="waiting",
            active_turn_id="",
            latest_event_id="",
        )
        try:
            from .web_model_tool_confirm_watcher import ensure_web_model_tool_confirm_watcher, start_web_model_browser_reload_window

            ensure_web_model_tool_confirm_watcher(group.group_id, aid, logger=_LOG)
            start_web_model_browser_reload_window(
                group.group_id,
                aid,
                reason="browser_delivery_pending",
                delivery_id=pending_delivery_id,
                turn_id=str(turn.get("turn_id") or ""),
                event_ids=list(turn.get("event_ids") or []),
                target_url=target_url,
            )
        except Exception:
            pass
        event = _append_delivery_event(
            group=group,
            actor_id=aid,
            turn=turn,
            kind="web_model.browser_delivery.pending",
            data={
                "provider": provider,
                "delivery_id": str(delivery_result.get("delivery_id") or turn.get("delivery_id") or ""),
                "trigger_event_id": str(trigger_event_id or "").strip(),
                "delivery_transport": "projected_session",
                "cursor_committed": bool(commit.get("cursor_committed")),
                "commit_error": "" if bool(commit.get("ok")) else str(commit.get("error") or ""),
                "bootstrap_seed": bool(bootstrap_seed),
                "target_url": target_url,
                "auto_bind_new_chat": bool(auto_bind_new_chat),
                "bound_conversation_url": "",
                "pending_conversation_url": True,
                "browser_surface": {
                    "state": str(browser_surface.get("state") or ""),
                    "url": str(browser_surface.get("url") or ""),
                },
                "browser": delivery_result.get("browser") if isinstance(delivery_result.get("browser"), dict) else {},
            },
        )
        return {
            "ok": True,
            "status": "target_chat_binding_pending",
            "turn_id": str(turn.get("turn_id") or ""),
            "cursor_committed": bool(commit.get("cursor_committed")),
            "commit": commit,
            "event": event,
            "delivery": delivery_result,
            "reschedule": False,
        }
    if ok:
        delivery_id = str(delivery_result.get("delivery_id") or turn.get("delivery_id") or "")
        try:
            from .web_model_tool_confirm_watcher import ensure_web_model_tool_confirm_watcher, start_web_model_browser_reload_window

            ensure_web_model_tool_confirm_watcher(group.group_id, aid, logger=_LOG)
            start_web_model_browser_reload_window(
                group.group_id,
                aid,
                reason="browser_delivery",
                delivery_id=delivery_id,
                turn_id=str(turn.get("turn_id") or ""),
                event_ids=list(turn.get("event_ids") or []),
                target_url=delivered_conversation_url or target_url,
            )
        except Exception:
            pass
        commit = commit_web_model_delivered_turn(group, actor_id=aid, turn=turn, by=aid)
        update_headless_state(
            group.group_id,
            aid,
            status="waiting",
            active_turn_id="",
            latest_event_id="",
        )
        if bootstrap_seed and not pending_conversation_url:
            _mark_bootstrap_seed_delivered(
                group.group_id,
                aid,
                target_url=delivered_conversation_url or target_url,
                seed_digest=seed_digest,
            )
        if auto_bind_new_chat and delivered_conversation_url:
            record_chatgpt_browser_state(
                group.group_id,
                aid,
                {
                    "conversation_url": delivered_conversation_url,
                    "pending_new_chat_bind": False,
                    "pending_new_chat_url": "",
                    "pending_new_chat_bind_started_at": "",
                    "pending_new_chat_submitted": False,
                    "pending_new_chat_submitted_at": "",
                    "pending_new_chat_delivery_id": "",
                    "pending_new_chat_last_turn_id": "",
                    "pending_new_chat_last_event_ids": [],
                    "pending_new_chat_last_tab_url": "",
                    "new_chat_bound_at": utc_now_iso(),
                    "last_error": "",
                },
            )
        record_chatgpt_browser_state(
            group.group_id,
            aid,
            {
                "last_delivery_at": utc_now_iso(),
                "last_turn_id": str(turn.get("turn_id") or ""),
                "last_event_ids": list(turn.get("event_ids") or []),
                "last_delivery_id": delivery_id,
                "last_delivery_status": "submitted",
                "last_submission_evidence": str(browser_result.get("submission_evidence") or ""),
                "last_send_selector": str(browser_result.get("send_selector") or ""),
                "last_tab_url": str(browser_result.get("tab_url") or ""),
                "last_error": "",
            },
        )
        event = _append_delivery_event(
            group=group,
            actor_id=aid,
            turn=turn,
            kind="web_model.browser_delivery.submitted",
            data={
                "provider": provider,
                "delivery_id": delivery_id,
                "trigger_event_id": str(trigger_event_id or "").strip(),
                "delivery_transport": "projected_session",
                "cursor_committed": bool(commit.get("cursor_committed")),
                "commit_error": "" if bool(commit.get("ok")) else str(commit.get("error") or ""),
                "bootstrap_seed": bool(bootstrap_seed),
                "target_url": target_url,
                "auto_bind_new_chat": bool(auto_bind_new_chat),
                "bound_conversation_url": delivered_conversation_url,
                "pending_conversation_url": bool(pending_conversation_url),
                "browser_surface": {
                    "state": str(browser_surface.get("state") or ""),
                    "url": str(browser_surface.get("url") or ""),
                },
                "browser": delivery_result.get("browser") if isinstance(delivery_result.get("browser"), dict) else {},
            },
        )
        return {
            "ok": True,
            "status": "submitted",
            "turn_id": str(turn.get("turn_id") or ""),
            "cursor_committed": bool(commit.get("cursor_committed")),
            "commit": commit,
            "event": event,
            "delivery": delivery_result,
            "reschedule": bool(commit.get("ok")) and bool(commit.get("cursor_committed")) and _has_unread_work(group, aid),
        }

    error = str(delivery_result.get("error") or "browser delivery failed")
    commit = commit_web_model_delivered_turn(group, actor_id=aid, turn=turn, by="system")
    record_chatgpt_browser_state(
        group.group_id,
        aid,
        {
            "last_delivery_at": utc_now_iso(),
            "last_turn_id": str(turn.get("turn_id") or ""),
            "last_event_ids": list(turn.get("event_ids") or []),
            "last_delivery_id": str(turn.get("delivery_id") or ""),
            "last_delivery_status": "failed",
            "last_submission_evidence": "",
            "last_send_selector": "",
            "last_error": error[:1200],
        },
    )
    update_headless_state(
        group.group_id,
        aid,
        status="waiting",
        active_turn_id="",
        latest_event_id="",
    )
    event = _append_delivery_event(
        group=group,
        actor_id=aid,
        turn=turn,
        kind="web_model.browser_delivery.failed",
        data={
            "provider": provider,
            "trigger_event_id": str(trigger_event_id or "").strip(),
            "delivery_id": str(turn.get("delivery_id") or ""),
            "error": error,
            "delivery_transport": "projected_session",
            "cursor_committed": bool(commit.get("cursor_committed")),
            "commit_error": "" if bool(commit.get("ok")) else str(commit.get("error") or ""),
            "target_url": target_url,
            "auto_bind_new_chat": bool(auto_bind_new_chat),
        },
    )
    return {
        "ok": False,
        "status": "failed",
        "turn_id": str(turn.get("turn_id") or ""),
        "error": error,
        "cursor_committed": bool(commit.get("cursor_committed")),
        "commit": commit,
        "event": event,
    }


def schedule_web_model_browser_delivery(
    *,
    group_id: str,
    actor_id: str,
    trigger_event_id: str = "",
    logger: Optional[logging.Logger] = None,
) -> bool:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return False
    key = (gid, aid)
    with _IN_FLIGHT_LOCK:
        if key in _IN_FLIGHT:
            return False
        _IN_FLIGHT.add(key)

    active_logger = logger or _LOG

    def _worker() -> None:
        reschedule = False
        try:
            result = submit_next_web_model_browser_turn(gid, aid, trigger_event_id=trigger_event_id)
            reschedule = bool(result.get("reschedule"))
            if not result.get("ok") and str(result.get("error") or "") != "browser_delivery_disabled":
                active_logger.info(
                    "[web-model-browser-delivery] failed group=%s actor=%s error=%s",
                    gid,
                    aid,
                    result.get("error"),
                )
        except Exception:
            active_logger.exception("[web-model-browser-delivery] unexpected error group=%s actor=%s", gid, aid)
        finally:
            with _IN_FLIGHT_LOCK:
                _IN_FLIGHT.discard(key)
        if reschedule:
            schedule_web_model_browser_delivery(group_id=gid, actor_id=aid, logger=active_logger)

    threading.Thread(
        target=_worker,
        name=f"cccc-web-model-browser-delivery-{gid}-{aid}",
        daemon=True,
    ).start()
    return True
