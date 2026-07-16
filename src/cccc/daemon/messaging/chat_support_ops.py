"""Shared helpers for chat operation flows."""

from __future__ import annotations

import logging
import threading
import time
from typing import Any, Callable, Dict, Optional


_HEADLESS_POST_WAKE_LOCK = threading.Lock()
_HEADLESS_POST_WAKE_IN_FLIGHT: set[tuple[str, str, str]] = set()


def _safe_log(logger: logging.Logger, level: str, message: str, *args: Any) -> None:
    method = getattr(logger, level, None)
    if not callable(method):
        return
    try:
        method(message, *args)
    except ValueError as exc:
        if "closed" in str(exc).lower():
            return
        raise


def auto_wake_recipients(
    group: Any,
    to: list[str],
    *,
    by: str,
    disabled_recipient_actor_ids: Callable[[Any, list[str]], list[str]],
    enabled_recipient_actor_ids: Callable[[Any, list[str]], list[str]],
    find_actor: Callable[[Any, str], Any],
    coerce_bool: Callable[..., bool],
    is_actor_running: Callable[[Any, str], bool],
    start_actor_process: Callable[..., Dict[str, Any]],
    update_actor: Callable[[Any, str, Dict[str, Any]], Any],
    runner_stop_actor: Callable[[str, str, str], Any],
    request_flush_pending_messages: Callable[..., bool],
    logger: logging.Logger,
    auto_wake_lock: threading.Lock,
    auto_wake_in_progress: set[tuple[str, str]],
) -> list[str]:
    """Best-effort background auto-start for recipients that are unavailable.

    Returns the actor IDs accepted for wake-up scheduling. If a wake for the
    same actor is already in progress, return that actor ID again so callers can
    still register post-wake delivery for the current message without starting a
    duplicate runtime boot.
    """
    scheduled: list[str] = []
    candidate_ids: list[str] = []
    seen_candidates: set[str] = set()

    for actor_id in disabled_recipient_actor_ids(group, to):
        aid = str(actor_id or "").strip()
        if not aid or aid == str(by or "").strip() or aid in seen_candidates:
            continue
        seen_candidates.add(aid)
        candidate_ids.append(aid)

    for actor_id in enabled_recipient_actor_ids(group, to):
        aid = str(actor_id or "").strip()
        if not aid or aid == str(by or "").strip() or aid in seen_candidates:
            continue
        if is_actor_running(group, aid):
            continue
        seen_candidates.add(aid)
        candidate_ids.append(aid)

    for actor_id in candidate_ids:
        key = (str(group.group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            continue
        with auto_wake_lock:
            if key in auto_wake_in_progress:
                scheduled.append(actor_id)
                continue
            auto_wake_in_progress.add(key)
        actor = find_actor(group, actor_id)
        if actor is None:
            with auto_wake_lock:
                auto_wake_in_progress.discard(key)
            continue
        was_enabled = coerce_bool(actor.get("enabled"), default=True)
        if was_enabled and is_actor_running(group, actor_id):
            with auto_wake_lock:
                auto_wake_in_progress.discard(key)
            continue
        cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
        env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
        runner_kind = str(actor.get("runner") or "pty").strip()
        runtime = str(actor.get("runtime") or "codex").strip()
        scheduled.append(actor_id)

        def _run_wake(
            *,
            wake_actor_id: str,
            wake_runner_kind: str,
            wake_runtime: str,
            wake_cmd: list[str],
            wake_env: dict[str, Any],
            wake_was_enabled: bool,
            wake_key: tuple[str, str],
        ) -> None:
            try:
                result = start_actor_process(
                    group,
                    wake_actor_id,
                    command=list(wake_cmd or []),
                    env=dict(wake_env or {}),
                    runner=wake_runner_kind,
                    runtime=wake_runtime,
                    by=by,
                )
                if result["success"]:
                    try:
                        if not wake_was_enabled:
                            update_actor(group, wake_actor_id, {"enabled": True})
                        request_flush_pending_messages(group, actor_id=wake_actor_id)
                    except Exception as e:
                        try:
                            runner_stop_actor(group.group_id, wake_actor_id, wake_runner_kind)
                        except Exception:
                            pass
                        logger.warning(
                            "[auto-wake] failed to persist enabled actor=%s group=%s: %s",
                            wake_actor_id,
                            group.group_id,
                            e,
                        )
                else:
                    logger.info(
                        "[auto-wake] actor start failed actor=%s group=%s err=%s",
                        wake_actor_id,
                        group.group_id,
                        result.get("error"),
                    )
            except Exception:
                logger.exception("[auto-wake] unexpected error actor=%s group=%s", wake_actor_id, group.group_id)
            finally:
                with auto_wake_lock:
                    auto_wake_in_progress.discard(wake_key)

        thread = threading.Thread(
            target=_run_wake,
            kwargs={
                "wake_actor_id": actor_id,
                "wake_runner_kind": runner_kind,
                "wake_runtime": runtime,
                "wake_cmd": list(cmd or []),
                "wake_env": dict(env or {}),
                "wake_was_enabled": bool(was_enabled),
                "wake_key": key,
            },
            name=f"cccc-auto-wake-{key[0]}-{key[1]}",
            daemon=True,
        )
        thread.start()
    return scheduled


def schedule_headless_post_wake_delivery(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    text: str,
    event_id: str,
    ts: str = "",
    reply_to: Optional[str] = None,
    attachments: Optional[list[dict[str, Any]]] = None,
    codex_actor_running: Callable[[str, str], bool],
    claude_actor_running: Callable[[str, str], bool],
    codex_submit_user_message: Callable[..., bool],
    claude_submit_user_message: Callable[..., bool],
    logger: logging.Logger,
    timeout_seconds: float = 30.0,
    poll_seconds: float = 0.2,
) -> bool:
    """Best-effort post-wake replay for headless chat delivery.

    The current send/reply flow appends the canonical chat event immediately,
    but a stopped headless actor cannot accept `submit_user_message(...)` yet.
    This helper waits for the freshly auto-started headless runtime to become
    running, then re-submits the exact canonical message payload once.
    """

    normalized_group_id = str(group_id or "").strip()
    normalized_actor_id = str(actor_id or "").strip()
    normalized_event_id = str(event_id or "").strip()
    normalized_runtime = str(runtime or "").strip().lower()
    clean_text = str(text or "")
    clean_ts = str(ts or "").strip()
    clean_reply_to = str(reply_to or "").strip() or None
    clean_attachments = [item for item in (attachments or []) if isinstance(item, dict)]
    if not normalized_group_id or not normalized_actor_id or not normalized_event_id:
        return False
    if normalized_runtime not in {"codex", "claude"}:
        return False
    if not clean_text.strip():
        return False

    key = (normalized_group_id, normalized_actor_id, normalized_event_id)
    with _HEADLESS_POST_WAKE_LOCK:
        if key in _HEADLESS_POST_WAKE_IN_FLIGHT:
            return False
        _HEADLESS_POST_WAKE_IN_FLIGHT.add(key)

    def _actor_running() -> bool:
        if normalized_runtime == "claude":
            return bool(claude_actor_running(normalized_group_id, normalized_actor_id))
        return bool(codex_actor_running(normalized_group_id, normalized_actor_id))

    def _submit() -> bool:
        if normalized_runtime == "claude":
            return bool(
                claude_submit_user_message(
                    group_id=normalized_group_id,
                    actor_id=normalized_actor_id,
                    text=clean_text,
                    event_id=normalized_event_id,
                    ts=clean_ts,
                    reply_to=clean_reply_to,
                    attachments=clean_attachments,
                )
            )
        return bool(
            codex_submit_user_message(
                group_id=normalized_group_id,
                actor_id=normalized_actor_id,
                text=clean_text,
                event_id=normalized_event_id,
                ts=clean_ts,
                reply_to=clean_reply_to,
                attachments=clean_attachments,
            )
        )

    def _worker() -> None:
        deadline = time.monotonic() + max(1.0, float(timeout_seconds))
        try:
            while time.monotonic() < deadline:
                if _actor_running() and _submit():
                    return
                time.sleep(max(0.05, float(poll_seconds)))
            _safe_log(
                logger,
                "info",
                "[headless-post-wake] timed out group=%s actor=%s event=%s runtime=%s",
                normalized_group_id,
                normalized_actor_id,
                normalized_event_id,
                normalized_runtime,
            )
        except Exception:
            _safe_log(
                logger,
                "exception",
                "[headless-post-wake] failed group=%s actor=%s event=%s runtime=%s",
                normalized_group_id,
                normalized_actor_id,
                normalized_event_id,
                normalized_runtime,
            )
        finally:
            with _HEADLESS_POST_WAKE_LOCK:
                _HEADLESS_POST_WAKE_IN_FLIGHT.discard(key)

    threading.Thread(
        target=_worker,
        name=f"cccc-headless-post-wake-{normalized_group_id}-{normalized_actor_id}-{normalized_event_id[:8]}",
        daemon=True,
    ).start()
    return True


def normalize_attachments(
    group: Any,
    raw: Any,
    *,
    resolve_blob_attachment_path: Callable[..., Any],
) -> list[dict[str, Any]]:
    if raw is None:
        return []
    if not isinstance(raw, list):
        raise ValueError("attachments must be a list")
    out: list[dict[str, Any]] = []
    for item in raw:
        if not isinstance(item, dict):
            raise ValueError("invalid attachment (must be object)")
        rel_path = str(item.get("path") or "").strip()
        if not rel_path:
            raise ValueError("attachment missing path")
        abs_path = resolve_blob_attachment_path(group, rel_path=rel_path)
        if not abs_path.exists() or not abs_path.is_file():
            raise ValueError(f"attachment not found: {rel_path}")
        try:
            size = int(abs_path.stat().st_size)
        except Exception:
            size = int(item.get("bytes") or 0)
        out.append(
            {
                "kind": str(item.get("kind") or "file"),
                "path": rel_path,
                "title": str(item.get("title") or ""),
                "mime_type": str(item.get("mime_type") or ""),
                "bytes": size,
                "sha256": str(item.get("sha256") or ""),
            }
        )
    return out
