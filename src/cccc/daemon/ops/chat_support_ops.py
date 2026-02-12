"""Shared helpers for chat operation flows."""

from __future__ import annotations

import logging
import threading
from typing import Any, Callable, Dict


def auto_wake_recipients(
    group: Any,
    to: list[str],
    *,
    by: str,
    disabled_recipient_actor_ids: Callable[[Any, list[str]], list[str]],
    find_actor: Callable[[Any, str], Any],
    coerce_bool: Callable[..., bool],
    start_actor_process: Callable[..., Dict[str, Any]],
    update_actor: Callable[[Any, str, Dict[str, Any]], Any],
    runner_stop_actor: Callable[[str, str, str], Any],
    logger: logging.Logger,
    auto_wake_lock: threading.Lock,
    auto_wake_in_progress: set[tuple[str, str]],
) -> list[str]:
    """Auto-start disabled actors that match the recipient list."""
    woken: list[str] = []
    disabled_ids = disabled_recipient_actor_ids(group, to)
    for actor_id in disabled_ids:
        key = (str(group.group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            continue
        with auto_wake_lock:
            if key in auto_wake_in_progress:
                continue
            auto_wake_in_progress.add(key)
        actor = find_actor(group, actor_id)
        if actor is None:
            with auto_wake_lock:
                auto_wake_in_progress.discard(key)
            continue
        try:
            if coerce_bool(actor.get("enabled"), default=True):
                continue
            cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
            env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
            runner_kind = str(actor.get("runner") or "pty").strip()
            runtime = str(actor.get("runtime") or "codex").strip()
            result = start_actor_process(
                group,
                actor_id,
                command=list(cmd or []),
                env=dict(env or {}),
                runner=runner_kind,
                runtime=runtime,
                by=by,
            )
            if result["success"]:
                try:
                    update_actor(group, actor_id, {"enabled": True})
                    woken.append(actor_id)
                except Exception as e:
                    try:
                        runner_stop_actor(group.group_id, actor_id, runner_kind)
                    except Exception:
                        pass
                    logger.warning(
                        "[auto-wake] failed to persist enabled actor=%s group=%s: %s",
                        actor_id,
                        group.group_id,
                        e,
                    )
            else:
                logger.info(
                    "[auto-wake] actor start failed actor=%s group=%s err=%s",
                    actor_id,
                    group.group_id,
                    result.get("error"),
                )
        except Exception:
            logger.exception("[auto-wake] unexpected error actor=%s group=%s", actor_id, group.group_id)
        finally:
            with auto_wake_lock:
                auto_wake_in_progress.discard(key)
    return woken


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
