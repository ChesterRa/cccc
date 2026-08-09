"""Atomic create-with-scope operation used by the Web single-request flow."""

from __future__ import annotations

import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel import active as active_store
from ...kernel import events
from ...kernel.group import attach_scope_to_group, create_group, delete_group
from ...kernel.ledger import append_event, notify_appended_event
from ...kernel.registry import load_registry
from ...kernel.scope import detect_scope
from ...paths import ensure_home

_CREATE_LOCK = threading.Lock()


@dataclass(frozen=True)
class _PreparedDirectory:
    path: Path
    created: bool


def _error(
    code: str, message: str, *, details: Dict[str, Any] | None = None
) -> DaemonResponse:
    return DaemonResponse(
        ok=False,
        error=DaemonError(code=code, message=message, details=details or {}),
    )


def _prepare_directory(raw_path: Any) -> _PreparedDirectory:
    if not isinstance(raw_path, str) or not raw_path.strip():
        raise ValueError("path must be a non-empty string")
    path = Path(raw_path.strip()).expanduser()
    if not path.is_absolute():
        raise ValueError("workspace path must be absolute")
    path = path.resolve()
    if path == ensure_home().resolve():
        raise ValueError("workspace scope must be a project directory, not CCCC_HOME")
    if path.exists():
        if not path.is_dir():
            raise NotADirectoryError(str(path))
        return _PreparedDirectory(path=path, created=False)
    if not path.parent.is_dir():
        raise FileNotFoundError(f"parent directory does not exist: {path.parent}")
    path.mkdir()
    return _PreparedDirectory(path=path, created=True)


def _remove_created_directory(prepared: _PreparedDirectory) -> list[str]:
    if not prepared.created:
        return []
    try:
        prepared.path.rmdir()
        return []
    except FileNotFoundError:
        return []
    except Exception as exc:
        return [f"workspace: {exc}"]


def _publish_created(group_id: str, title: str) -> None:
    try:
        events.publish_event("group.created", {"group_id": group_id, "title": title})
    except Exception:
        return


def _notify_committed(event: Dict[str, Any]) -> None:
    try:
        notify_appended_event(event)
    except Exception:
        return


def _rollback(
    group_id: str,
    prepared: _PreparedDirectory,
    *,
    scope_key: str,
    previous_default: str,
    previous_active: str,
    restore_active: bool,
) -> list[str]:
    failures: list[str] = []
    if restore_active:
        try:
            active_store.set_active_group_id(previous_active)
        except Exception as exc:
            failures.append(f"active: {exc}")
    try:
        delete_group(load_registry(), group_id=group_id, publish=False)
    except Exception as exc:
        failures.append(f"group: {exc}")
    try:
        registry = load_registry()
        if previous_default and not str(registry.defaults.get(scope_key) or "").strip():
            registry.defaults[scope_key] = previous_default
            registry.save()
    except Exception as exc:
        failures.append(f"scope default: {exc}")
    registry = load_registry()
    if group_id in registry.groups or group_id in registry.defaults.values():
        failures.append("registry still references created group")
    failures.extend(_remove_created_directory(prepared))
    return failures


def create_group_with_scope(args: Dict[str, Any]) -> DaemonResponse:
    try:
        prepared = _prepare_directory(args.get("path"))
    except FileNotFoundError as exc:
        return _error("path_not_found", str(exc))
    except NotADirectoryError as exc:
        return _error("not_dir", str(exc))
    except PermissionError as exc:
        return _error("permission_denied", str(exc))
    except (OSError, ValueError) as exc:
        return _error("invalid_scope_path", str(exc))

    with _CREATE_LOCK:
        group_id = ""
        scope_key = ""
        previous_default = ""
        restore_active = False
        previous_active = str(active_store.load_active().get("active_group_id") or "")
        try:
            scope = detect_scope(prepared.path)
            scope_key = scope.scope_key
            registry = load_registry()
            previous_default = str(registry.defaults.get(scope_key) or "").strip()
            group = create_group(
                registry,
                title=str(args.get("title") or "working-group"),
                topic=str(args.get("topic") or ""),
                publish=False,
            )
            group_id = group.group_id
            group = attach_scope_to_group(registry, group, scope, set_active=True)
            event = append_event(
                group.ledger_path,
                kind="group.create",
                group_id=group_id,
                scope_key=scope.scope_key,
                by=str(args.get("by") or "user"),
                data={
                    "title": group.doc.get("title", ""),
                    "topic": group.doc.get("topic", ""),
                },
                notify=False,
            )
            restore_active = True
            active_store.set_active_group_id(group_id)
        except Exception as exc:
            if not group_id:
                failures = _remove_created_directory(prepared)
            else:
                failures = _rollback(
                    group_id,
                    prepared,
                    scope_key=scope_key,
                    previous_default=previous_default,
                    previous_active=previous_active,
                    restore_active=restore_active,
                )
            if failures:
                return _error(
                    "rollback_failed",
                    f"{exc}; rollback failed: {'; '.join(failures)}",
                    details={"original_code": "group_create_with_scope_failed"},
                )
            return _error("group_create_with_scope_failed", str(exc))

    _notify_committed(event)
    _publish_created(group_id, str(group.doc.get("title") or ""))
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "scope_key": scope.scope_key,
            "title": group.doc.get("title"),
            "group": group.doc,
            "event": event,
        },
    )
