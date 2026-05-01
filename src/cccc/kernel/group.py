from __future__ import annotations

import copy
import errno
import hashlib
import logging
import shutil
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional

import yaml  # type: ignore

from ..paths import ensure_home
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso
from .ledger_segments import ensure_ledger_layout
from .registry import Registry
from .scope import ScopeIdentity

_DEFAULT_AUTOMATION_STANDUP_SNIPPET = """{{interval_minutes}} minutes have passed. Stand-up checkpoint (foreman only).

Use MCP chat for any visible update. Keep this short.

Goal:
1. Collect exactly three short lines from each active owner: current status, next step, blocker.
2. Treat this as a coordination interrupt, not a task switch.
3. Do not answer from fuzzy memory. If current status, next step, or blocker is not grounded in fresh context, use MCP tool `cccc_bootstrap`, follow `memory_recall_gate`, and open `cccc_help` before replying.
4. After the checkpoint, return to your prior active task unless priority changed or a real blocker appeared.

Only update shared state if the checkpoint changed shared truth.
"""

LOGGER = logging.getLogger(__name__)

_DEFAULT_AUTOMATION_BUILTIN_SNIPPETS = {
    "standup": _DEFAULT_AUTOMATION_STANDUP_SNIPPET,
}


def _normalize_automation_snippet_map(raw: Any) -> Dict[str, str]:
    out: Dict[str, str] = {}
    if not isinstance(raw, dict):
        return out
    for key_raw, value_raw in raw.items():
        if not isinstance(key_raw, str):
            continue
        key = key_raw.strip()
        if not key or not isinstance(value_raw, str):
            continue
        out[key] = value_raw
    return out


def default_automation_builtin_snippets() -> Dict[str, str]:
    return copy.deepcopy(_DEFAULT_AUTOMATION_BUILTIN_SNIPPETS)


def split_automation_snippets_for_storage(snippets: Any) -> tuple[Dict[str, str], Dict[str, str]]:
    built_in = default_automation_builtin_snippets()
    normalized = _normalize_automation_snippet_map(snippets)
    custom: Dict[str, str] = {}
    built_in_overrides: Dict[str, str] = {}
    for key, value in normalized.items():
        if key in built_in:
            if value != built_in[key]:
                built_in_overrides[key] = value
            continue
        custom[key] = value
    return custom, built_in_overrides


def normalize_automation_snippet_storage(automation: Any) -> tuple[Dict[str, str], Dict[str, str]]:
    built_in = default_automation_builtin_snippets()
    raw_custom = _normalize_automation_snippet_map(automation.get("snippets") if isinstance(automation, dict) else {})
    raw_overrides = _normalize_automation_snippet_map(
        automation.get("snippet_overrides") if isinstance(automation, dict) else {}
    )

    custom: Dict[str, str] = {key: value for key, value in raw_overrides.items() if key not in built_in}
    custom.update({key: value for key, value in raw_custom.items() if key not in built_in})
    built_in_overrides: Dict[str, str] = {}

    for key, value in raw_custom.items():
        if key not in built_in:
            continue
        if value != built_in[key]:
            built_in_overrides[key] = value

    for key, value in raw_overrides.items():
        if key not in built_in:
            continue
        if value != built_in[key]:
            built_in_overrides[key] = value
        else:
            built_in_overrides.pop(key, None)

    return custom, built_in_overrides


def stored_automation_snippets(automation: Any) -> Dict[str, str]:
    custom, built_in_overrides = normalize_automation_snippet_storage(automation)
    out = dict(custom)
    out.update(built_in_overrides)
    return out


def effective_automation_snippets(automation: Any) -> Dict[str, str]:
    built_in = default_automation_builtin_snippets()
    custom, built_in_overrides = normalize_automation_snippet_storage(automation)
    out = dict(built_in)
    out.update(built_in_overrides)
    out.update(custom)
    return out


def automation_snippet_catalog(automation: Any) -> Dict[str, Dict[str, str]]:
    custom, built_in_overrides = normalize_automation_snippet_storage(automation)
    return {
        "built_in": default_automation_builtin_snippets(),
        "built_in_overrides": built_in_overrides,
        "custom": custom,
    }


def _default_automation_ruleset() -> Dict[str, Any]:
    # Stored in group.yaml; must not share mutable objects across groups.
    return {
        "version": 1,
        "rules": [
            {
                "id": "standup",
                "enabled": False,
                "scope": "group",
                "owner_actor_id": None,
                "to": ["@foreman"],
                "trigger": {"kind": "interval", "every_seconds": 900},
                "action": {
                    "kind": "notify",
                    "priority": "normal",
                    "requires_ack": False,
                    "title": "Stand-up reminder",
                    "snippet_ref": "standup",
                    "message": "",
                },
            }
        ],
        "snippets": {},
        "snippet_overrides": {},
    }


def default_automation_ruleset_doc() -> Dict[str, Any]:
    """Return a fresh default automation ruleset document."""
    return copy.deepcopy(_default_automation_ruleset())


def _new_group_id(seed: str) -> str:
    h = hashlib.sha256(seed.encode("utf-8")).hexdigest()
    return "g_" + h[:12]


def _random_group_id() -> str:
    return "g_" + uuid.uuid4().hex[:12]


@dataclass
class Group:
    group_id: str
    path: Path
    doc: Dict[str, Any]

    @property
    def ledger_path(self) -> Path:
        ensure_ledger_layout(self.path)
        return self.path / "ledger.jsonl"

    def save(self) -> None:
        self.doc.setdefault("v", 1)
        self.doc["updated_at"] = utc_now_iso()
        atomic_write_text(self.path / "group.yaml", yaml.safe_dump(self.doc, allow_unicode=True, sort_keys=False))


def load_group(group_id: str) -> Optional[Group]:
    home = ensure_home()
    gp = home / "groups" / group_id
    p = gp / "group.yaml"
    if not p.exists():
        return None
    try:
        doc = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
        if not isinstance(doc, dict):
            return None
        ensure_ledger_layout(gp)
        return Group(group_id=group_id, path=gp, doc=doc)
    except Exception:
        return None


def create_group(reg: Registry, *, title: str, topic: str = "") -> Group:
    home = ensure_home()
    groups_dir = home / "groups"
    groups_dir.mkdir(parents=True, exist_ok=True)

    now = utc_now_iso()
    group_id = _random_group_id()
    gp = groups_dir / group_id
    gp.mkdir(parents=True, exist_ok=True)
    (gp / "context").mkdir(parents=True, exist_ok=True)
    (gp / "scopes").mkdir(parents=True, exist_ok=True)
    (gp / "state").mkdir(parents=True, exist_ok=True)
    ensure_ledger_layout(gp)

    group_doc: Dict[str, Any] = {
        "v": 1,
        "group_id": group_id,
        "title": title.strip() if title.strip() else "working-group",
        "topic": topic.strip(),
        "created_at": now,
        "updated_at": now,
        "running": False,
        "state": "active",  # active / idle / paused
        "active_scope_key": "",
        "scopes": [],
        "actors": [],
        # Single-layer storage: automation rules/snippets live in group.yaml under CCCC_HOME.
        "automation": default_automation_ruleset_doc(),
    }
    atomic_write_text(gp / "group.yaml", yaml.safe_dump(group_doc, allow_unicode=True, sort_keys=False))

    reg.groups[group_id] = {
        "group_id": group_id,
        "title": group_doc["title"],
        "topic": group_doc["topic"],
        "path": str(gp),
        "default_scope_key": "",
        "created_at": now,
        "updated_at": now,
    }
    reg.save()

    # Publish event for SSE subscribers
    from .events import publish_event
    publish_event("group.created", {"group_id": group_id, "title": group_doc["title"]})

    return Group(group_id=group_id, path=gp, doc=group_doc)


def attach_scope_to_group(reg: Registry, group: Group, scope: ScopeIdentity, *, set_active: bool = True) -> Group:
    now = utc_now_iso()

    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        scopes = []
        group.doc["scopes"] = scopes

    existing: Optional[Dict[str, Any]] = None
    for item in scopes:
        if isinstance(item, dict) and item.get("scope_key") == scope.scope_key:
            existing = item
            break

    scope_entry = existing if existing is not None else {}
    scope_entry.update(
        {
            "scope_key": scope.scope_key,
            "url": scope.url,
            "label": scope.label,
            "git_remote": scope.git_remote,
        }
    )
    if existing is None:
        scopes.append(scope_entry)

    scope_dir = group.path / "scopes" / scope.scope_key
    scope_dir.mkdir(parents=True, exist_ok=True)
    scope_yaml = scope_dir / "scope.yaml"
    created_at = now
    if scope_yaml.exists():
        try:
            prior = yaml.safe_load(scope_yaml.read_text(encoding="utf-8")) or {}
            if isinstance(prior, dict) and isinstance(prior.get("created_at"), str) and prior.get("created_at"):
                created_at = prior["created_at"]
        except Exception as e:
            LOGGER.warning("failed to parse scope metadata; resetting created_at: scope=%s path=%s err=%s", scope.scope_key, scope_yaml, e)
    scope_doc: Dict[str, Any] = {
        "v": 1,
        "scope_key": scope.scope_key,
        "url": scope.url,
        "label": scope.label,
        "git_remote": scope.git_remote,
        "created_at": created_at,
        "updated_at": now,
    }
    atomic_write_text(scope_yaml, yaml.safe_dump(scope_doc, allow_unicode=True, sort_keys=False))

    if set_active or not str(group.doc.get("active_scope_key") or "").strip():
        group.doc["active_scope_key"] = scope.scope_key

    group.save()

    reg.defaults[scope.scope_key] = group.group_id
    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["title"] = group.doc.get("title") or meta.get("title") or ""
        meta["default_scope_key"] = group.doc.get("active_scope_key") or meta.get("default_scope_key") or ""
        meta["updated_at"] = now
    reg.save()
    return group


def set_active_scope(reg: Registry, group: Group, *, scope_key: str) -> Group:
    wanted = scope_key.strip()
    if not wanted:
        raise ValueError("missing scope_key")

    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        scopes = []

    ok = any(isinstance(item, dict) and item.get("scope_key") == wanted for item in scopes)
    if not ok:
        raise ValueError(f"scope not attached: {wanted}")

    group.doc["active_scope_key"] = wanted
    group.save()

    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["default_scope_key"] = wanted
        meta["updated_at"] = group.doc.get("updated_at") or utc_now_iso()
    reg.save()
    return group


def ensure_group_for_scope(reg: Registry, scope: ScopeIdentity) -> Group:
    home = ensure_home()
    groups_dir = home / "groups"
    groups_dir.mkdir(parents=True, exist_ok=True)

    existing_id = (reg.defaults.get(scope.scope_key) or "").strip()
    if existing_id:
        g = load_group(existing_id)
        if g is not None:
            return attach_scope_to_group(reg, g, scope, set_active=True)

    seed = scope.git_remote or scope.url
    group_id = _new_group_id(seed)
    g = load_group(group_id)
    if g is not None:
        return attach_scope_to_group(reg, g, scope, set_active=True)

    now = utc_now_iso()
    gp = groups_dir / group_id
    gp.mkdir(parents=True, exist_ok=True)
    (gp / "context").mkdir(parents=True, exist_ok=True)
    (gp / "scopes").mkdir(parents=True, exist_ok=True)
    (gp / "state").mkdir(parents=True, exist_ok=True)
    (gp / "ledger.jsonl").touch(exist_ok=True)

    group_doc: Dict[str, Any] = {
        "v": 1,
        "group_id": group_id,
        "title": scope.label,
        "topic": "",
        "created_at": now,
        "updated_at": now,
        "running": False,
        "state": "active",  # active / idle / paused
        "active_scope_key": "",
        "scopes": [],
        "actors": [],
        # Keep deterministic defaults consistent with create_group().
        "automation": default_automation_ruleset_doc(),
    }
    atomic_write_text(gp / "group.yaml", yaml.safe_dump(group_doc, allow_unicode=True, sort_keys=False))

    reg.groups[group_id] = {
        "group_id": group_id,
        "title": scope.label,
        "topic": "",
        "path": str(gp),
        "default_scope_key": "",
        "created_at": now,
        "updated_at": now,
    }
    reg.save()

    return attach_scope_to_group(reg, Group(group_id=group_id, path=gp, doc=group_doc), scope, set_active=True)


def update_group(reg: Registry, group: Group, *, patch: Dict[str, Any]) -> Group:
    if "title" in patch:
        title = str(patch.get("title") or "").strip()
        if title:
            group.doc["title"] = title

    if "topic" in patch:
        topic = str(patch.get("topic") or "").strip()
        group.doc["topic"] = topic

    group.save()

    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["title"] = str(group.doc.get("title") or meta.get("title") or "")
        meta["topic"] = str(group.doc.get("topic") or "")
        meta["updated_at"] = str(group.doc.get("updated_at") or utc_now_iso())
    reg.save()
    # Publish event for SSE subscribers (best-effort invalidation for group list).
    from .events import publish_event

    publish_event(
        "group.updated",
        {
            "group_id": group.group_id,
            "title": str(group.doc.get("title") or ""),
            "topic": str(group.doc.get("topic") or ""),
        },
    )
    return group


def detach_scope_from_group(reg: Registry, group: Group, *, scope_key: str) -> Group:
    wanted = scope_key.strip()
    if not wanted:
        raise ValueError("missing scope_key")

    scopes = group.doc.get("scopes")
    if not isinstance(scopes, list):
        scopes = []

    before = len(scopes)
    scopes = [s for s in scopes if not (isinstance(s, dict) and str(s.get("scope_key") or "") == wanted)]
    if len(scopes) == before:
        raise ValueError(f"scope not attached: {wanted}")
    group.doc["scopes"] = scopes

    if str(group.doc.get("active_scope_key") or "") == wanted:
        new_active = ""
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            k = str(sc.get("scope_key") or "").strip()
            if k:
                new_active = k
                break
        group.doc["active_scope_key"] = new_active

    try:
        shutil.rmtree(group.path / "scopes" / wanted)
    except FileNotFoundError:
        pass
    except Exception as e:
        LOGGER.warning("failed to remove detached scope directory: group=%s scope=%s err=%s", group.group_id, wanted, e)

    if reg.defaults.get(wanted) == group.group_id:
        reg.defaults.pop(wanted, None)

    group.save()

    meta = reg.groups.get(group.group_id)
    if isinstance(meta, dict):
        meta["default_scope_key"] = str(group.doc.get("active_scope_key") or "")
        meta["updated_at"] = str(group.doc.get("updated_at") or utc_now_iso())
    reg.save()
    return group


def delete_group(reg: Registry, *, group_id: str) -> None:
    gid = group_id.strip()
    if not gid:
        raise ValueError("missing group_id")

    home = ensure_home()
    gp = home / "groups" / gid
    if gp.exists():
        _delete_group_dir(gp)

    reg.groups.pop(gid, None)
    for k, v in list(reg.defaults.items()):
        if v == gid:
            reg.defaults.pop(k, None)
    reg.save()

    # Publish event for SSE subscribers
    from .events import publish_event
    publish_event("group.deleted", {"group_id": gid})


def _delete_group_dir(path: Path) -> None:
    """Delete group directory robustly under concurrent background writes.

    In fast-moving runtimes, transient files may appear while deleting.
    We retry a few times for ENOTEMPTY/EBUSY-like races, then quarantine
    the directory name to unblock user-facing group deletion.
    """
    transient_errno = {errno.ENOTEMPTY, errno.EBUSY, errno.EACCES, errno.EPERM}
    last_exc: Optional[BaseException] = None
    for attempt in range(4):
        try:
            shutil.rmtree(path)
            return
        except FileNotFoundError:
            return
        except OSError as e:
            last_exc = e
            if e.errno not in transient_errno:
                raise
            time.sleep(0.05 * (attempt + 1))

    # Last resort: move out of the canonical group_id path so user-level delete
    # can complete even if an external process keeps writing briefly.
    quarantine = path.with_name(f".deleting-{path.name}-{uuid.uuid4().hex[:8]}")
    try:
        path.rename(quarantine)
    except FileNotFoundError:
        return
    except Exception:
        if last_exc is not None:
            raise last_exc
        raise

    try:
        shutil.rmtree(quarantine, ignore_errors=True)
    except Exception:
        pass

    if quarantine.exists():
        LOGGER.warning("group deletion quarantined path not fully removed yet: %s", quarantine)


def get_group_state(group: Group) -> str:
    """Get the current group state.

    `stopped` is a durable lifecycle state written by `group_stop`. Unlike
    `idle`/`paused`, callers should not assume runtimes still exist.
    """
    state = str(group.doc.get("state") or "active").strip()
    if state not in ("active", "idle", "paused", "stopped"):
        return "active"
    return state


def set_group_state(group: Group, *, state: str) -> Group:
    """Set the group state.
    
    States:
    - active: Normal operation, all automation enabled
    - idle: Task completed; internal automation is muted while user-defined scheduled rules may still run
    - paused: User paused, all automation disabled

    This helper intentionally does not accept `stopped`. Stopping a group has
    runtime side effects and must go through the lifecycle stop path.
    
    State transitions:
    - active -> idle: Foreman judges task complete
    - idle -> active: New message or task arrives
    - active -> paused: User manually pauses
    - paused -> active: User manually resumes
    """
    s = state.strip().lower()
    if s not in ("active", "idle", "paused"):
        raise ValueError(f"invalid state: {state} (must be active/idle/paused; use group_stop for stopped)")
    
    group.doc["state"] = s
    group.save()
    # Publish event for SSE subscribers (best-effort invalidation for group list).
    from .events import publish_event

    publish_event("group.state_changed", {"group_id": group.group_id, "state": s})
    return group
