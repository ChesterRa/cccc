"""Registry and group-list operation handlers for daemon."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ...kernel.group import load_group
from ...kernel.query_projections import get_groups_projection
from ...kernel.registry import load_registry
from ...paths import ensure_home
from ..runner_state_ops import web_model_group_running
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _registry_group_yaml_path(group_id: str, meta: Dict[str, Any]) -> Path:
    """Best-effort path resolution for a group's group.yaml from registry metadata."""
    gid = str(group_id or "").strip()
    raw_path = str(meta.get("path") or "").strip() if isinstance(meta, dict) else ""
    if raw_path:
        return Path(raw_path).expanduser() / "group.yaml"
    return ensure_home() / "groups" / gid / "group.yaml"


def _registry_group_health(group_id: str, meta: Dict[str, Any]) -> tuple[str, Any]:
    """Classify registry entry health.

    Returns:
      ("ok", Group) when loadable
      ("missing", None) when group.yaml is absent
      ("corrupt", None) when group.yaml exists but cannot be loaded
    """
    path = _registry_group_yaml_path(group_id, meta)
    if not path.exists():
        return "missing", None
    group = load_group(group_id)
    if group is None:
        return "corrupt", None
    return "ok", group


def handle_groups(_: Dict[str, Any]) -> DaemonResponse:
    projection = get_groups_projection()
    groups = projection.get("groups") if isinstance(projection.get("groups"), list) else []
    out = []
    for group_meta in groups:
        if not isinstance(group_meta, dict):
            continue
        gid = str(group_meta.get("group_id") or "").strip()
        running = (
            (codex_app_supervisor.group_running(gid) if gid else False)
            or
            (claude_app_supervisor.group_running(gid) if gid else False)
            or
            (pty_runner.SUPERVISOR.group_running(gid) if gid else False)
            or (headless_runner.SUPERVISOR.group_running(gid) if gid else False)
            or (web_model_group_running(gid) if gid else False)
        )
        item = dict(group_meta)
        item["running"] = bool(running)
        out.append(item)
    return DaemonResponse(
        ok=True,
        result={
            "groups": out,
            "registry_health": dict(projection.get("registry_health") or {}),
        },
    )


def handle_registry_reconcile(args: Dict[str, Any]) -> DaemonResponse:
    remove_missing = coerce_bool(args.get("remove_missing"), default=False)
    reg = load_registry()
    entries = list(reg.groups.items())
    missing_ids: list[str] = []
    corrupt_ids: list[str] = []
    for gid, meta in entries:
        group_id = str(gid or "").strip()
        if not group_id:
            continue
        meta_dict = meta if isinstance(meta, dict) else {}
        health, _ = _registry_group_health(group_id, meta_dict)
        if health == "missing":
            missing_ids.append(group_id)
        elif health == "corrupt":
            corrupt_ids.append(group_id)

    removed_group_ids: list[str] = []
    removed_default_scope_keys: list[str] = []
    if remove_missing and missing_ids:
        to_remove = set(missing_ids)
        for gid in list(to_remove):
            if reg.groups.pop(gid, None) is not None:
                removed_group_ids.append(gid)
        if removed_group_ids:
            removed_set = set(removed_group_ids)
            for sk, gid in list(reg.defaults.items()):
                if str(gid or "").strip() in removed_set:
                    reg.defaults.pop(sk, None)
                    removed_default_scope_keys.append(str(sk))
            reg.save()

    return DaemonResponse(
        ok=True,
        result={
            "dry_run": not remove_missing,
            "scanned_groups": len(entries),
            "missing_group_ids": sorted(missing_ids),
            "corrupt_group_ids": sorted(corrupt_ids),
            "removed_group_ids": sorted(removed_group_ids),
            "removed_default_scope_keys": sorted(removed_default_scope_keys),
        },
    )


def try_handle_registry_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "groups":
        return handle_groups(args)
    if op == "registry_reconcile":
        return handle_registry_reconcile(args)
    return None
