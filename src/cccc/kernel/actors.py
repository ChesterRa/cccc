from __future__ import annotations

import re
from typing import Any, Dict, List, Optional

from ..contracts.v1 import Actor, ActorRole, ActorSubmit, RunnerKind, AgentRuntime
from ..util.time import utc_now_iso
from ..util.conv import coerce_bool
from .group import Group
from .runtime import get_runtime_command_with_flags

# Actor ID naming rules:
# - Length: 1-32 characters
# - Allowed: Unicode letters, numbers, CJK characters, hyphen (-), underscore (_)
# - First char: must be letter, number, or CJK character (not - or _)
# - Forbidden: spaces, dots, @, /, \, and other special symbols
# Pattern breakdown:
#   [\w\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af] - first char: word char or CJK
#   [\w\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af_-]{0,31} - rest: same + hyphen/underscore
_ACTOR_ID_RE = re.compile(
    r"^[\w\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]"
    r"[\w\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af_-]{0,31}$"
)

# Reserved IDs that cannot be used as actor names
_RESERVED_IDS = frozenset({
    "user", "all", "system", "foreman", "peers", "admin", "root", "cccc",
    "@all", "@peers", "@foreman", "@user",
})


def validate_actor_id(actor_id: str) -> str:
    """Validate and normalize actor ID. Returns normalized ID or raises ValueError."""
    aid = actor_id.strip()
    
    if not aid:
        raise ValueError("Please enter a name")
    
    if len(aid) > 32:
        raise ValueError("Name must be 32 characters or less")

    if aid[0] in ("-", "_"):
        raise ValueError("Name must start with a letter or number")
    
    if " " in aid or "\t" in aid or "\n" in aid:
        raise ValueError("Name cannot contain spaces")
    
    if "." in aid:
        raise ValueError("Name cannot contain dots")
    
    if "@" in aid:
        raise ValueError("Name cannot contain @")
    
    if "/" in aid or "\\" in aid:
        raise ValueError("Name cannot contain slashes")
    
    if not _ACTOR_ID_RE.match(aid):
        raise ValueError(
            "Name can only contain letters, numbers, hyphens, and underscores"
        )
    
    if aid.casefold() in _RESERVED_IDS or aid in _RESERVED_IDS:
        raise ValueError(f"'{aid}' is reserved, please use another name")
    
    return aid


def generate_actor_id(group: Group, prefix: str = "agent", runtime: str = "") -> str:
    """Generate a unique actor ID.
    
    If runtime is provided, uses runtime as prefix (e.g., claude-1, codex-1).
    Otherwise uses the provided prefix (default: agent-1, agent-2).
    """
    # Use runtime as prefix if provided and valid
    if runtime:
        prefix = runtime
    
    existing = {str(a.get("id", "")) for a in list_actors(group)}
    for i in range(1, 1000):
        candidate = f"{prefix}-{i}"
        if candidate not in existing:
            return candidate
    # Fallback (should never happen)
    import uuid
    return f"{prefix}-{uuid.uuid4().hex[:8]}"


def _ensure_actor_list(group: Group) -> List[Dict[str, Any]]:
    actors = group.doc.get("actors")
    if not isinstance(actors, list):
        actors = []
        group.doc["actors"] = actors
    return actors


def list_actors(group: Group) -> List[Dict[str, Any]]:
    actors = _ensure_actor_list(group)
    out: List[Dict[str, Any]] = []
    for item in actors:
        if isinstance(item, dict) and isinstance(item.get("id"), str) and item.get("id"):
            out.append(item)
    return out


def find_actor(group: Group, actor_id: str) -> Optional[Dict[str, Any]]:
    wanted = actor_id.strip()
    if not wanted:
        return None
    for item in list_actors(group):
        if item.get("id") == wanted:
            return item
    return None


def get_effective_role(group: Group, actor_id: str) -> ActorRole:
    """Get the effective role of an actor based on position.
    
    Role is automatically determined:
    - First enabled actor in the list = foreman
    - All other actors = peer
    
    This ensures every group always has a foreman (if it has any actors).
    """
    wanted = actor_id.strip()
    if not wanted:
        return "peer"
    
    actors = list_actors(group)
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        # Skip disabled actors when determining foreman
        if not coerce_bool(actor.get("enabled"), default=True):
            continue
        aid = str(actor.get("id") or "").strip()
        if not aid:
            continue
        # First enabled actor is foreman
        if aid == wanted:
            return "foreman"
        else:
            # We found the first enabled actor and it's not the wanted one
            # So the wanted one must be a peer
            break
    
    return "peer"


def find_foreman(group: Group) -> Optional[Dict[str, Any]]:
    """Find the current foreman (first enabled actor).
    
    Note: This is now based on position, not stored role.
    """
    actors = list_actors(group)
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        if not coerce_bool(actor.get("enabled"), default=True):
            continue
        aid = str(actor.get("id") or "").strip()
        if aid:
            return actor
    return None


def add_actor(
    group: Group,
    *,
    actor_id: str,
    title: str = "",
    command: Optional[List[str]] = None,
    env: Optional[Dict[str, str]] = None,
    default_scope_key: str = "",
    submit: ActorSubmit = "enter",
    enabled: bool = True,
    runner: RunnerKind = "pty",
    runtime: AgentRuntime = "codex",
) -> Dict[str, Any]:
    """Add a new actor to the group.
    
    Role is automatically determined by position:
    - If this is the first enabled actor, it becomes foreman
    - Otherwise, it's a peer
    """
    # Validate actor ID using new rules (supports Unicode)
    aid = validate_actor_id(actor_id)

    existing = find_actor(group, aid)
    if existing is not None:
        raise ValueError(f"Name already exists: {aid}")

    now = utc_now_iso()
    actor = Actor(
        id=aid,
        role=None,  # Role is auto-determined, not stored
        title=title.strip() if title else "",
        command=list(command or []),
        env=dict(env or {}),
        default_scope_key=default_scope_key.strip(),
        submit=submit,
        enabled=coerce_bool(enabled, default=True),
        runner=runner,
        runtime=runtime,
        created_at=now,
        updated_at=now,
    )
    _ensure_actor_list(group).append(actor.model_dump(exclude_none=True))
    group.save()
    
    # Return with effective role included
    result = actor.model_dump(exclude_none=True)
    result["role"] = get_effective_role(group, aid)
    return result


def remove_actor(group: Group, actor_id: str) -> None:
    aid = actor_id.strip()
    if not aid:
        raise ValueError("missing actor id")

    actors = _ensure_actor_list(group)
    before = len(actors)
    actors[:] = [a for a in actors if not (isinstance(a, dict) and a.get("id") == aid)]
    if len(actors) == before:
        raise ValueError(f"actor not found: {aid}")
    group.save()


def reorder_actors(group: Group, actor_ids: List[str]) -> List[Dict[str, Any]]:
    """Reorder actors. The first actor becomes foreman.
    
    Args:
        actor_ids: List of actor IDs in desired order
        
    Returns:
        Reordered actor list with effective roles
    """
    actors = list_actors(group)
    id_to_actor = {str(a.get("id")): a for a in actors}
    
    # Validate all IDs exist
    for aid in actor_ids:
        if aid not in id_to_actor:
            raise ValueError(f"actor not found: {aid}")
    
    # Check no duplicates
    if len(actor_ids) != len(set(actor_ids)):
        raise ValueError("duplicate actor IDs in list")
    
    # Check all actors are included
    if set(actor_ids) != set(id_to_actor.keys()):
        raise ValueError("actor_ids must include all actors")
    
    # Reorder
    new_actors = [id_to_actor[aid] for aid in actor_ids]
    group.doc["actors"] = new_actors
    group.save()
    
    # Return with effective roles
    result = []
    for actor in new_actors:
        a = dict(actor)
        a["role"] = get_effective_role(group, str(actor.get("id") or ""))
        result.append(a)
    return result


def update_actor(group: Group, actor_id: str, patch: Dict[str, Any]) -> Dict[str, Any]:
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id.strip()}")

    # Note: 'role' in patch is ignored - role is auto-determined by position
    if "role" in patch:
        pass  # Silently ignore for backward compatibility

    if "title" in patch:
        title = str(patch.get("title") or "").strip()
        item["title"] = title

    if "command" in patch:
        cmd = patch.get("command")
        if cmd is None:
            item["command"] = []
        elif isinstance(cmd, list) and all(isinstance(x, str) for x in cmd):
            item["command"] = cmd
        else:
            raise ValueError("invalid command")
        if (
            str(item.get("runtime") or "").strip() == "custom"
            and str(item.get("runner") or "pty").strip() != "headless"
            and not item.get("command")
        ):
            raise ValueError("custom runtime requires a command (PTY runner)")

    if "env" in patch:
        env = patch.get("env")
        if env is None:
            item["env"] = {}
        elif isinstance(env, dict) and all(isinstance(k, str) and isinstance(v, str) for k, v in env.items()):
            item["env"] = env
        else:
            raise ValueError("invalid env")

    if "default_scope_key" in patch:
        item["default_scope_key"] = str(patch.get("default_scope_key") or "").strip()

    if "submit" in patch:
        submit = patch.get("submit")
        if submit is None:
            item["submit"] = "enter"
        elif submit in ("enter", "newline", "none"):
            item["submit"] = submit
        else:
            raise ValueError("invalid submit")

    if "enabled" in patch:
        item["enabled"] = coerce_bool(patch.get("enabled"), default=False)

    if "runner" in patch:
        runner = patch.get("runner")
        if runner is None:
            item["runner"] = "pty"
        elif runner in ("pty", "headless"):
            item["runner"] = runner
        else:
            raise ValueError("invalid runner (must be 'pty' or 'headless')")

    if "runtime" in patch:
        runtime = patch.get("runtime")
        if runtime is None:
            item["runtime"] = "codex"
        elif runtime in ("amp", "auggie", "claude", "codex", "cursor", "droid", "neovate", "gemini", "kilocode", "opencode", "copilot", "custom"):
            item["runtime"] = runtime
        else:
            raise ValueError("invalid runtime")
        if (
            str(item.get("runtime") or "").strip() == "custom"
            and str(item.get("runner") or "pty").strip() != "headless"
            and not item.get("command")
        ):
            raise ValueError("custom runtime requires a command (PTY runner)")

    # Normalize "empty command" for non-custom PTY runtimes: treat it as "use default command".
    runner_kind = str(item.get("runner") or "pty").strip() or "pty"
    runtime_name = str(item.get("runtime") or "codex").strip() or "codex"
    if runner_kind != "headless" and runtime_name != "custom" and not item.get("command"):
        item["command"] = get_runtime_command_with_flags(runtime_name)

    item["updated_at"] = utc_now_iso()
    group.save()
    
    # Return with effective role
    result = dict(item)
    result["role"] = get_effective_role(group, actor_id)
    return result


def resolve_recipient_tokens(group: Group, tokens: List[str]) -> List[str]:
    raw: List[str] = []
    for t in tokens:
        if not isinstance(t, str):
            continue
        s = t.strip()
        if not s:
            continue
        raw.append(s)

    if not raw:
        return []

    actors = list_actors(group)
    id_set = {str(a.get("id")) for a in actors if isinstance(a, dict) and isinstance(a.get("id"), str)}
    title_map: Dict[str, List[str]] = {}
    for a in actors:
        if not isinstance(a, dict):
            continue
        aid = a.get("id")
        title = a.get("title")
        if not isinstance(aid, str) or not aid.strip():
            continue
        if not isinstance(title, str) or not title.strip():
            continue
        key = title.strip().casefold()
        title_map.setdefault(key, []).append(aid.strip())

    def _canonical_one(token: str) -> str:
        t = token.strip()
        if not t:
            return ""

        # IM-like mentions: allow "@peer-a" / "@Claude" by stripping leading "@"
        if t.startswith("@") and t not in ("@all", "@peers", "@foreman", "@user"):
            t = t[1:].strip()
            if not t:
                # Ignore stray "@" tokens instead of raising "unknown recipient: ".
                return ""

        if t in ("@all", "@peers", "@foreman"):
            return t
        if t in ("user", "@user"):
            return "user"

        if t in id_set:
            return t

        key = t.casefold()
        ids = title_map.get(key) or []
        if len(ids) == 1:
            return ids[0]
        if len(ids) > 1:
            raise ValueError(f"ambiguous recipient title: {t}")

        raise ValueError(f"unknown recipient: {t}")

    out: List[str] = []
    seen = set()
    for t in raw:
        c = _canonical_one(t)
        if not c:
            continue
        if c in seen:
            continue
        seen.add(c)
        out.append(c)
    return out
