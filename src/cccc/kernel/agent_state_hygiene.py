from __future__ import annotations

import hashlib
import json
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional

from ..util.time import parse_utc_iso

_MIND_FIELDS = ("environment_summary", "user_model", "persona_notes")
_EXECUTION_FIELDS = ("active_task_id", "focus", "next_action", "what_changed", "blockers")


def _container_value(container: Any, key: str) -> Any:
    if isinstance(container, dict):
        return container.get(key)
    return getattr(container, key, None)


def _text_value(container: Any, key: str) -> str:
    return str(_container_value(container, key) or "").strip()


def _list_value(container: Any, key: str) -> List[str]:
    raw = _container_value(container, key)
    if not isinstance(raw, list):
        return []
    out: List[str] = []
    for item in raw:
        text = str(item or "").strip()
        if text:
            out.append(text)
    return out


def _trim_text(value: Any, *, max_chars: int) -> str:
    text = str(value or "").strip()
    if len(text) <= max_chars:
        return text
    if max_chars <= 3:
        return text[:max_chars]
    return text[: max_chars - 3].rstrip() + "..."


def build_mind_context_mini(warm: Any, *, max_chars: int = 96) -> Dict[str, str]:
    mini = {
        "environment_summary": _trim_text(_text_value(warm, "environment_summary"), max_chars=max_chars),
        "user_model": _trim_text(_text_value(warm, "user_model"), max_chars=max_chars),
        "persona_notes": _trim_text(_text_value(warm, "persona_notes"), max_chars=max_chars),
    }
    return {key: value for key, value in mini.items() if value}


def build_mind_context_fingerprint(warm: Any) -> str:
    payload = {
        "environment_summary": _text_value(warm, "environment_summary"),
        "user_model": _text_value(warm, "user_model"),
        "persona_notes": _text_value(warm, "persona_notes"),
    }
    if not any(payload.values()):
        return ""
    raw = json.dumps(payload, ensure_ascii=False, sort_keys=True)
    return hashlib.sha1(raw.encode("utf-8")).hexdigest()


def sync_mind_context_runtime_state(
    actor_runtime: Dict[str, Any],
    *,
    warm: Any,
    updated_at: Any,
    now: Optional[datetime] = None,
) -> bool:
    now_dt = now or datetime.now(timezone.utc)
    current_hash = build_mind_context_fingerprint(warm)
    current_updated_at = str(updated_at or "").strip() or now_dt.isoformat().replace("+00:00", "Z")
    stored_hash = str(actor_runtime.get("mind_context_hash") or "").strip()
    stored_seen_updated_at = str(actor_runtime.get("agent_state_last_seen_updated_at") or "").strip()
    changed = False

    if current_hash and current_hash != stored_hash:
        actor_runtime["mind_context_hash"] = current_hash
        actor_runtime["mind_context_touched_at"] = current_updated_at
        actor_runtime["agent_state_last_seen_updated_at"] = current_updated_at
        actor_runtime["hot_only_updates_since_mind_touch"] = 0
        return True

    if not current_hash:
        if stored_hash:
            actor_runtime["mind_context_hash"] = ""
            changed = True
        if actor_runtime.get("mind_context_touched_at"):
            actor_runtime["mind_context_touched_at"] = ""
            changed = True
        if actor_runtime.get("hot_only_updates_since_mind_touch") not in (None, "", 0):
            actor_runtime["hot_only_updates_since_mind_touch"] = 0
            changed = True
        if stored_seen_updated_at != current_updated_at:
            actor_runtime["agent_state_last_seen_updated_at"] = current_updated_at
            changed = True
        return changed

    if not stored_hash:
        actor_runtime["mind_context_hash"] = current_hash
        actor_runtime["mind_context_touched_at"] = current_updated_at
        actor_runtime["agent_state_last_seen_updated_at"] = current_updated_at
        actor_runtime["hot_only_updates_since_mind_touch"] = 0
        return True

    if stored_seen_updated_at != current_updated_at:
        actor_runtime["agent_state_last_seen_updated_at"] = current_updated_at
        try:
            current_count = int(actor_runtime.get("hot_only_updates_since_mind_touch") or 0)
        except Exception:
            current_count = 0
        actor_runtime["hot_only_updates_since_mind_touch"] = current_count + 1
        changed = True
    return changed


def evaluate_agent_state_hygiene(
    *,
    actor_id: str,
    hot: Any,
    warm: Any,
    updated_at: Any,
    mind_touched_at: Any = None,
    hot_only_updates_since_mind_touch: int = 0,
    present: bool = True,
    now: Optional[datetime] = None,
    stale_after_seconds: int = 20 * 60,
    mind_stale_after_seconds: int = 20 * 60,
    mind_hot_only_update_threshold: int = 3,
) -> Dict[str, Any]:
    aid = str(actor_id or "").strip()
    now_dt = now or datetime.now(timezone.utc)
    updated = str(updated_at or "").strip()
    age_seconds: Optional[int] = None
    if updated:
        dt = parse_utc_iso(updated)
        if dt is not None:
            age_seconds = max(0, int((now_dt - dt).total_seconds()))
    execution_is_stale = (age_seconds is None) or (age_seconds > int(stale_after_seconds))

    mind_touch_raw = str(mind_touched_at or "").strip()
    if (not mind_touch_raw) and any(_text_value(warm, field) for field in _MIND_FIELDS):
        mind_touch_raw = updated
    mind_touch_age_seconds: Optional[int] = None
    if mind_touch_raw:
        dt = parse_utc_iso(mind_touch_raw)
        if dt is not None:
            mind_touch_age_seconds = max(0, int((now_dt - dt).total_seconds()))

    exec_present_fields: List[str] = []
    if _text_value(hot, "active_task_id"):
        exec_present_fields.append("active_task_id")
    if _text_value(hot, "focus"):
        exec_present_fields.append("focus")
    if _text_value(hot, "next_action"):
        exec_present_fields.append("next_action")
    if _text_value(warm, "what_changed"):
        exec_present_fields.append("what_changed")
    if _list_value(hot, "blockers"):
        exec_present_fields.append("blockers")

    mind_present_fields: List[str] = [field for field in _MIND_FIELDS if _text_value(warm, field)]
    mind_is_stale_by_age = (mind_touch_age_seconds is None) or (mind_touch_age_seconds > int(mind_stale_after_seconds))
    mind_is_stale_by_churn = int(hot_only_updates_since_mind_touch or 0) >= int(mind_hot_only_update_threshold)

    if not present:
        execution_status = "missing"
        mind_status = "missing"
    else:
        execution_status = "missing" if not exec_present_fields else "stale" if execution_is_stale else "ready"
        if not mind_present_fields:
            mind_status = "missing"
        elif mind_is_stale_by_age or mind_is_stale_by_churn:
            mind_status = "stale"
        elif len(mind_present_fields) < len(_MIND_FIELDS):
            mind_status = "partial"
        else:
            mind_status = "ready"

    recommendation = "update_agent_state_now"
    if present:
        if execution_status == "missing":
            recommendation = "fill_execution_state"
        elif execution_status == "stale":
            recommendation = "refresh_execution_state"
        elif mind_status in {"missing", "partial"}:
            recommendation = "fill_mind_context"
        elif mind_status == "stale":
            recommendation = "refresh_mind_context"
        else:
            recommendation = "state_healthy"

    return {
        "actor_id": aid,
        "present": bool(present),
        "age_seconds": age_seconds,
        "stale": bool(execution_is_stale),
        "min_fields_ready": bool(exec_present_fields),
        "execution_health": {
            "status": execution_status,
            "present_fields": exec_present_fields,
            "missing_fields": [field for field in _EXECUTION_FIELDS if field not in exec_present_fields],
        },
        "mind_context_health": {
            "status": mind_status,
            "present_fields": mind_present_fields,
            "missing_fields": [field for field in _MIND_FIELDS if field not in mind_present_fields],
            "touched_at": mind_touch_raw or None,
            "touch_age_seconds": mind_touch_age_seconds,
            "hot_only_updates_since_touch": int(hot_only_updates_since_mind_touch or 0),
        },
        "update_command": (
            'cccc_agent_state(action="update", actor_id="<self>", focus="...", next_action="...", '
            'what_changed="...", environment_summary="...", user_model="...", persona_notes="...")'
        ),
        "execution_update_command": (
            'cccc_agent_state(action="update", actor_id="<self>", focus="...", next_action="...", what_changed="...")'
        ),
        "mind_context_update_command": (
            'cccc_agent_state(action="update", actor_id="<self>", environment_summary="...", '
            'user_model="...", persona_notes="...")'
        ),
        "recommendation": recommendation,
    }
