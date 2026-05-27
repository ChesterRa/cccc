"""Group settings operations for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.delivery_policy import auto_mark_on_delivery_from_doc, coerce_auto_mark_on_delivery
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.messaging import get_default_send_to
from ...kernel.pet_actor import PET_ACTOR_ID, get_pet_actor
from ...kernel.permissions import require_group_permission
from ...kernel.terminal_transcript import apply_terminal_transcript_patch, get_terminal_transcript_settings
from ..actors.actor_profile_runtime import resolve_linked_actor_before_start
from ..pet.pet_runtime_ops import (
    capture_pet_actor_state,
    is_pet_actor_running,
    pet_runtime_changed,
    restore_pet_actor_state,
    stop_pet_actor_runtime,
    sync_pet_actor_from_foreman,
)
from ..pet.review_scheduler import cancel_pet_review, request_pet_review
from ..pet.profile_refresh import maybe_request_pet_profile_refresh
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _group_settings_error_details(exc: Exception) -> Optional[Dict[str, Any]]:
    message = str(exc or "").strip()
    if not message:
        return None
    if message == "desktop pet requires a foreman actor":
        return {"reason": "desktop_pet_requires_foreman"}
    if message.startswith("failed to start pet actor:"):
        cause = message.partition(":")[2].strip()
        return {"reason": "pet_actor_start_failed", "cause": cause}
    if message == "failed to start pet actor":
        return {"reason": "pet_actor_start_failed"}
    if message.startswith("pet start failed and rollback restart failed:"):
        cause = message.partition(":")[2].strip()
        return {"reason": "pet_actor_rollback_restart_failed", "cause": cause}
    return None


def _safe_int(value: Any, *, default: int, min_value: int = 0, max_value: Optional[int] = None) -> int:
    try:
        out = int(value)
    except Exception:
        out = int(default)
    if out < int(min_value):
        out = int(min_value)
    if max_value is not None and out > int(max_value):
        out = int(max_value)
    return out


def handle_group_settings_update(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    start_actor_process: Callable[..., dict[str, Any]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    messaging_keys = {"default_send_to"}
    delivery_keys = {"min_interval_seconds", "auto_mark_on_delivery"}
    automation_keys = {
        "nudge_after_seconds",
        "reply_required_nudge_after_seconds",
        "attention_ack_nudge_after_seconds",
        "unread_nudge_after_seconds",
        "nudge_digest_min_interval_seconds",
        "nudge_max_repeats_per_obligation",
        "nudge_escalate_after_repeats",
        "actor_idle_timeout_seconds",
        "keepalive_delay_seconds",
        "keepalive_max_per_actor",
        "silence_timeout_seconds",
        "help_nudge_interval_seconds",
        "help_nudge_min_messages",
    }
    terminal_transcript_keys = {
        "terminal_transcript_visibility",
        "terminal_transcript_notify_tail",
        "terminal_transcript_notify_lines",
    }
    feature_keys = {"panorama_enabled", "desktop_pet_enabled"}
    allowed = messaging_keys | delivery_keys | automation_keys | terminal_transcript_keys | feature_keys

    unknown = set(patch.keys()) - allowed
    if unknown:
        return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)})
    if not patch:
        return _error("invalid_patch", "empty patch")
    if "default_send_to" in patch:
        value = str(patch.get("default_send_to") or "").strip()
        if value not in ("foreman", "broadcast"):
            return _error(
                "invalid_patch",
                "default_send_to must be 'foreman' or 'broadcast'",
                details={"default_send_to": value},
            )
    try:
        require_group_permission(group, by=by, action="group.settings_update")
        pet_review_after_save = False

        messaging_patch = {k: v for k, v in patch.items() if k in messaging_keys}
        if messaging_patch:
            messaging = group.doc.get("messaging") if isinstance(group.doc.get("messaging"), dict) else {}
            messaging["default_send_to"] = str(messaging_patch.get("default_send_to") or "foreman").strip()
            group.doc["messaging"] = messaging

        delivery_patch = {k: v for k, v in patch.items() if k in delivery_keys}
        if delivery_patch:
            delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
            for key, value in delivery_patch.items():
                if key == "auto_mark_on_delivery":
                    delivery[key] = coerce_auto_mark_on_delivery(value)
                else:
                    delivery[key] = int(value)
            group.doc["delivery"] = delivery

        automation_patch = {k: v for k, v in patch.items() if k in automation_keys}
        if automation_patch:
            automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
            for key, value in automation_patch.items():
                automation[key] = int(value)
            group.doc["automation"] = automation

        tt_patch: Dict[str, Any] = {}
        if "terminal_transcript_visibility" in patch:
            tt_patch["visibility"] = patch.get("terminal_transcript_visibility")
        if "terminal_transcript_notify_tail" in patch:
            tt_patch["notify_tail"] = patch.get("terminal_transcript_notify_tail")
        if "terminal_transcript_notify_lines" in patch:
            tt_patch["notify_lines"] = patch.get("terminal_transcript_notify_lines")
        if tt_patch:
            apply_terminal_transcript_patch(group.doc, tt_patch)

        features_patch = {k: v for k, v in patch.items() if k in feature_keys}
        if features_patch:
            features = group.doc.get("features") if isinstance(group.doc.get("features"), dict) else {}
            pet_actor_before = get_pet_actor(group) if "desktop_pet_enabled" in features_patch else None
            pet_state_before = (
                capture_pet_actor_state(group, load_actor_private_env=load_actor_private_env)
                if "desktop_pet_enabled" in features_patch
                else None
            )
            pet_was_running = is_pet_actor_running(
                group,
                actor=pet_actor_before,
                effective_runner_kind=effective_runner_kind,
            ) if "desktop_pet_enabled" in features_patch else False
            desktop_pet_enabled_before = coerce_bool(features.get("desktop_pet_enabled"), default=False)
            if "panorama_enabled" in features_patch:
                features["panorama_enabled"] = coerce_bool(features_patch["panorama_enabled"], default=False)
            if "desktop_pet_enabled" in features_patch:
                features["desktop_pet_enabled"] = coerce_bool(features_patch["desktop_pet_enabled"], default=False)
            group.doc["features"] = features
            if "desktop_pet_enabled" in features_patch:
                try:
                    desired_enabled = coerce_bool(features_patch["desktop_pet_enabled"], default=False)
                    resolve_before_start = lambda grp, aid, caller_id="", is_admin=False: resolve_linked_actor_before_start(
                        grp,
                        aid,
                        get_actor_profile=get_actor_profile,
                        load_actor_profile_secrets=load_actor_profile_secrets,
                        update_actor_private_env=update_actor_private_env,
                        caller_id=caller_id,
                        is_admin=is_admin,
                    )
                    if not desired_enabled and isinstance(pet_actor_before, dict):
                        stop_pet_actor_runtime(
                            group,
                            actor=pet_actor_before,
                            by=by,
                            effective_runner_kind=effective_runner_kind,
                            remove_headless_state=remove_headless_state,
                            remove_pty_state_if_pid=remove_pty_state_if_pid,
                            emit_event=pet_was_running,
                        )
                        cancel_pet_review(group.group_id)
                        sync_pet_actor_from_foreman(
                            group,
                            effective_runner_kind=effective_runner_kind,
                            load_actor_private_env=load_actor_private_env,
                            update_actor_private_env=update_actor_private_env,
                            delete_actor_private_env=delete_actor_private_env,
                            resolve_linked_actor_before_start=resolve_before_start,
                            caller_id=str(args.get("caller_id") or "").strip(),
                            is_admin=coerce_bool(args.get("is_admin"), default=False),
                        )
                    else:
                        sync_pet_actor_from_foreman(
                            group,
                            effective_runner_kind=effective_runner_kind,
                            load_actor_private_env=load_actor_private_env,
                            update_actor_private_env=update_actor_private_env,
                            delete_actor_private_env=delete_actor_private_env,
                            resolve_linked_actor_before_start=resolve_before_start,
                            caller_id=str(args.get("caller_id") or "").strip(),
                            is_admin=coerce_bool(args.get("is_admin"), default=False),
                        )
                        pet_actor_after = get_pet_actor(group)
                        pet_private_env_after = load_actor_private_env(group.group_id, PET_ACTOR_ID)
                        if coerce_bool(group.doc.get("running"), default=False) and isinstance(pet_actor_after, dict):
                            pet_private_env_before = (
                                pet_state_before.get("private_env")
                                if isinstance(pet_state_before, dict) and isinstance(pet_state_before.get("private_env"), dict)
                                else {}
                            )
                            pet_actor_before_doc = (
                                pet_state_before.get("actor_doc")
                                if isinstance(pet_state_before, dict) and isinstance(pet_state_before.get("actor_doc"), dict)
                                else None
                            )
                            config_changed = pet_runtime_changed(
                                pet_actor_before_doc,
                                pet_actor_after,
                                before_private_env=pet_private_env_before,
                                after_private_env=pet_private_env_after,
                            )
                            if pet_was_running and config_changed:
                                stop_pet_actor_runtime(
                                    group,
                                    actor=pet_actor_before,
                                    by=by,
                                    effective_runner_kind=effective_runner_kind,
                                    remove_headless_state=remove_headless_state,
                                    remove_pty_state_if_pid=remove_pty_state_if_pid,
                                    emit_event=True,
                                )
                            should_start = (not pet_was_running) or config_changed
                            if should_start:
                                start_result = start_actor_process(
                                    group,
                                    PET_ACTOR_ID,
                                    command=list(pet_actor_after.get("command") or []),
                                    env=dict(pet_actor_after.get("env") or {}),
                                    runner=str(pet_actor_after.get("runner") or "pty"),
                                    runtime=str(pet_actor_after.get("runtime") or "codex"),
                                    by=by,
                                )
                                if not bool(start_result.get("success")):
                                    start_error = str(start_result.get("error") or "").strip()
                                    if start_error:
                                        raise RuntimeError(f"failed to start pet actor: {start_error}")
                                    raise RuntimeError("failed to start pet actor")
                            pet_review_after_save = True
                except Exception:
                    features["desktop_pet_enabled"] = desktop_pet_enabled_before
                    group.doc["features"] = features
                    restored_actor = restore_pet_actor_state(
                        group,
                        pet_state_before,
                        update_actor_private_env=update_actor_private_env,
                        delete_actor_private_env=delete_actor_private_env,
                    )
                    if desktop_pet_enabled_before and pet_was_running and isinstance(restored_actor, dict):
                        restart_result = start_actor_process(
                            group,
                            PET_ACTOR_ID,
                            command=list(restored_actor.get("command") or []),
                            env=dict(restored_actor.get("env") or {}),
                            runner=str(restored_actor.get("runner") or "pty"),
                            runtime=str(restored_actor.get("runtime") or "codex"),
                            by=by,
                        )
                        if not bool(restart_result.get("success")):
                            raise RuntimeError(
                                f"pet start failed and rollback restart failed: {restart_result.get('error') or 'unknown error'}"
                            )
                    raise

        group.save()
        if pet_review_after_save:
            try:
                request_pet_review(group.group_id, reason="pet_enabled", immediate=True)
            except Exception:
                pass
            try:
                maybe_request_pet_profile_refresh(group.group_id, reason="pet_enabled")
            except Exception:
                pass
    except Exception as e:
        return _error("group_settings_update_failed", str(e), details=_group_settings_error_details(e))

    automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
    delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
    features = group.doc.get("features") if isinstance(group.doc.get("features"), dict) else {}
    tt = get_terminal_transcript_settings(group.doc)
    settings = {
        "default_send_to": get_default_send_to(group.doc),
        "nudge_after_seconds": _safe_int(automation.get("nudge_after_seconds", 300), default=300, min_value=0),
        "reply_required_nudge_after_seconds": _safe_int(
            automation.get("reply_required_nudge_after_seconds", 300),
            default=300,
            min_value=0,
        ),
        "attention_ack_nudge_after_seconds": _safe_int(
            automation.get("attention_ack_nudge_after_seconds", 600),
            default=600,
            min_value=0,
        ),
        "unread_nudge_after_seconds": _safe_int(automation.get("unread_nudge_after_seconds", 900), default=900, min_value=0),
        "nudge_digest_min_interval_seconds": _safe_int(
            automation.get("nudge_digest_min_interval_seconds", 120),
            default=120,
            min_value=0,
        ),
        "nudge_max_repeats_per_obligation": _safe_int(
            automation.get("nudge_max_repeats_per_obligation", 3),
            default=3,
            min_value=0,
        ),
        "nudge_escalate_after_repeats": _safe_int(
            automation.get("nudge_escalate_after_repeats", 2),
            default=2,
            min_value=0,
        ),
        "actor_idle_timeout_seconds": _safe_int(
            automation.get("actor_idle_timeout_seconds", 0),
            default=0,
            min_value=0,
        ),
        "keepalive_delay_seconds": _safe_int(automation.get("keepalive_delay_seconds", 120), default=120, min_value=0),
        "keepalive_max_per_actor": _safe_int(automation.get("keepalive_max_per_actor", 3), default=3, min_value=0),
        "silence_timeout_seconds": _safe_int(automation.get("silence_timeout_seconds", 0), default=0, min_value=0),
        "help_nudge_interval_seconds": _safe_int(
            automation.get("help_nudge_interval_seconds", 600),
            default=600,
            min_value=0,
        ),
        "help_nudge_min_messages": _safe_int(automation.get("help_nudge_min_messages", 10), default=10, min_value=0),
        "min_interval_seconds": _safe_int(delivery.get("min_interval_seconds", 0), default=0, min_value=0),
        "auto_mark_on_delivery": auto_mark_on_delivery_from_doc(delivery),
        "terminal_transcript_visibility": str(tt.get("visibility") or "foreman"),
        "terminal_transcript_notify_tail": coerce_bool(tt.get("notify_tail"), default=False),
        "terminal_transcript_notify_lines": _safe_int(
            tt.get("notify_lines", 20),
            default=20,
            min_value=1,
            max_value=80,
        ),
        "panorama_enabled": coerce_bool(features.get("panorama_enabled"), default=False),
        "desktop_pet_enabled": coerce_bool(features.get("desktop_pet_enabled"), default=False),
    }

    event = append_event(
        group.ledger_path,
        kind="group.settings_update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"patch": dict(patch)},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "settings": settings, "event": event})


def try_handle_group_settings_op(
    op: str,
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    start_actor_process: Callable[..., dict[str, Any]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
) -> Optional[DaemonResponse]:
    if op == "group_settings_update":
        return handle_group_settings_update(
            args,
            effective_runner_kind=effective_runner_kind,
            start_actor_process=start_actor_process,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
            delete_actor_private_env=delete_actor_private_env,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
        )
    return None
