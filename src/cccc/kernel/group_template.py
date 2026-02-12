from __future__ import annotations

import shlex
from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Tuple

import yaml  # type: ignore

from ..contracts.v1.automation import AutomationRuleSet
from ..contracts.v1.group_template import GroupTemplate, GroupTemplateActor
from .group import Group
from .prompt_files import (
    DEFAULT_PREAMBLE_BODY,
    HELP_FILENAME,
    PREAMBLE_FILENAME,
    load_builtin_help_markdown,
    read_group_prompt_file,
)
from .terminal_transcript import get_terminal_transcript_settings
from .messaging import get_default_send_to
from ..util.time import utc_now_iso
from ..util.conv import coerce_bool


def _normalize_command(cmd: Any) -> List[str]:
    if cmd is None:
        return []
    if isinstance(cmd, list):
        return [str(x).strip() for x in cmd if isinstance(x, str) and str(x).strip()]
    if isinstance(cmd, str):
        s = cmd.strip()
        return shlex.split(s) if s else []
    return []


def _as_int(v: Any, default: int) -> int:
    try:
        return int(v)
    except Exception:
        return int(default)


def parse_group_template(text: str) -> GroupTemplate:
    raw = str(text or "").strip()
    if not raw:
        raise ValueError("template is empty")
    try:
        data = yaml.safe_load(raw)
    except Exception as e:
        raise ValueError(f"invalid template YAML: {e}") from e
    if not isinstance(data, dict):
        raise ValueError("template must be a mapping (YAML object)")
    return GroupTemplate.model_validate(data)


def dump_group_template(template: GroupTemplate) -> str:
    payload = template.model_dump(by_alias=True, exclude_none=True)
    if payload.get("prompts") == {}:
        payload.pop("prompts", None)
    return yaml.safe_dump(payload, allow_unicode=True, sort_keys=False, default_flow_style=False)


def build_group_template_from_group(group: Group, *, cccc_version: str = "") -> GroupTemplate:
    title = str(group.doc.get("title") or "")
    topic = str(group.doc.get("topic") or "")

    actors_in = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
    actors: list[dict[str, Any]] = []
    for a in actors_in:
        if not isinstance(a, dict):
            continue
        aid = str(a.get("id") or "").strip()
        if not aid:
            continue
        actors.append(
            {
                "id": aid,
                "title": str(a.get("title") or ""),
                "runtime": str(a.get("runtime") or "codex"),
                "runner": str(a.get("runner") or "pty"),
                "command": list(a.get("command") or []) if isinstance(a.get("command"), list) else [],
                "submit": str(a.get("submit") or "enter"),
                "enabled": coerce_bool(a.get("enabled"), default=True),
            }
        )

    automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
    delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
    tt = get_terminal_transcript_settings(group.doc)
    default_send_to = get_default_send_to(group.doc)

    settings: dict[str, Any] = {
        "default_send_to": default_send_to,
        "nudge_after_seconds": _as_int(automation.get("nudge_after_seconds", 300), 300),
        "reply_required_nudge_after_seconds": _as_int(automation.get("reply_required_nudge_after_seconds", 300), 300),
        "attention_ack_nudge_after_seconds": _as_int(automation.get("attention_ack_nudge_after_seconds", 600), 600),
        "unread_nudge_after_seconds": _as_int(automation.get("unread_nudge_after_seconds", 900), 900),
        "nudge_digest_min_interval_seconds": _as_int(automation.get("nudge_digest_min_interval_seconds", 120), 120),
        "nudge_max_repeats_per_obligation": _as_int(automation.get("nudge_max_repeats_per_obligation", 3), 3),
        "nudge_escalate_after_repeats": _as_int(automation.get("nudge_escalate_after_repeats", 2), 2),
        "auto_mark_on_delivery": coerce_bool(delivery.get("auto_mark_on_delivery"), default=False),
        "actor_idle_timeout_seconds": _as_int(automation.get("actor_idle_timeout_seconds", 600), 600),
        "keepalive_delay_seconds": _as_int(automation.get("keepalive_delay_seconds", 120), 120),
        "keepalive_max_per_actor": _as_int(automation.get("keepalive_max_per_actor", 3), 3),
        "silence_timeout_seconds": _as_int(automation.get("silence_timeout_seconds", 600), 600),
        "help_nudge_interval_seconds": _as_int(automation.get("help_nudge_interval_seconds", 600), 600),
        "help_nudge_min_messages": _as_int(automation.get("help_nudge_min_messages", 10), 10),
        "min_interval_seconds": _as_int(delivery.get("min_interval_seconds", 0), 0),
        "terminal_transcript_visibility": str(tt.get("visibility") or "foreman"),
        "terminal_transcript_notify_tail": coerce_bool(tt.get("notify_tail"), default=True),
        "terminal_transcript_notify_lines": _as_int(tt.get("notify_lines", 20), 20),
    }

    def _prompt_value(filename: str) -> Optional[str]:
        pf = read_group_prompt_file(group, filename)
        if not pf.found:
            return None
        if pf.content is None:
            raise ValueError(f"failed to read group prompt file: {filename}")
        if not isinstance(pf.content, str) or not pf.content.strip():
            return None
        return str(pf.content)

    prompts = {
        "preamble": _prompt_value(PREAMBLE_FILENAME),
        "help": _prompt_value(HELP_FILENAME),
    }

    ruleset: AutomationRuleSet
    raw_rules = automation.get("rules") if isinstance(automation.get("rules"), list) else []
    raw_snippets = automation.get("snippets") if isinstance(automation.get("snippets"), dict) else {}
    try:
        ruleset = AutomationRuleSet.model_validate({"rules": raw_rules, "snippets": raw_snippets})
    except Exception:
        ruleset = AutomationRuleSet()

    return GroupTemplate(
        kind="cccc.group_template",
        v=1,
        title=title,
        topic=topic,
        exported_at=utc_now_iso(),
        cccc_version=str(cccc_version or ""),
        actors=[GroupTemplateActor.model_validate(a) for a in actors],
        settings=settings,  # type: ignore[arg-type]
        prompts=prompts,  # type: ignore[arg-type]
        automation=ruleset,
    )


@dataclass(frozen=True)
class GroupTemplateDiff:
    actors_add: List[str]
    actors_update: List[str]
    actors_remove: List[str]
    settings_changed: Dict[str, Tuple[Any, Any]]
    prompts_changed: Dict[str, Tuple[bool, int, int, str, str]]


def preview_group_template_replace(group: Group, template: GroupTemplate) -> GroupTemplateDiff:
    existing = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
    existing_ids = [str(a.get("id") or "").strip() for a in existing if isinstance(a, dict)]
    existing_ids = [x for x in existing_ids if x]
    template_ids = [str(a.actor_id or "").strip() for a in (template.actors or []) if str(a.actor_id or "").strip()]

    existing_set = set(existing_ids)
    template_set = set(template_ids)

    add_ids = [aid for aid in template_ids if aid not in existing_set]
    remove_ids = [aid for aid in existing_ids if aid not in template_set]

    update_ids: list[str] = []
    existing_map: dict[str, dict[str, Any]] = {}
    for a in existing:
        if isinstance(a, dict) and isinstance(a.get("id"), str) and a.get("id"):
            existing_map[str(a["id"])] = a
    for a in template.actors or []:
        aid = str(a.actor_id or "").strip()
        if not aid or aid not in existing_map:
            continue
        cur = existing_map[aid]
        changed = False
        if str(cur.get("title") or "") != str(a.title or ""):
            changed = True
        if str(cur.get("runtime") or "codex") != str(a.runtime or "codex"):
            changed = True
        if str(cur.get("runner") or "pty") != str(a.runner or "pty"):
            changed = True
        if coerce_bool(cur.get("enabled"), default=True) != coerce_bool(a.enabled, default=True):
            changed = True
        if str(cur.get("submit") or "enter") != str(a.submit or "enter"):
            changed = True
        cur_cmd = cur.get("command")
        cur_cmd_list = [str(x).strip() for x in cur_cmd] if isinstance(cur_cmd, list) else []
        new_cmd_list = _normalize_command(a.command)
        if cur_cmd_list != new_cmd_list:
            changed = True
        if changed:
            update_ids.append(aid)

    # Settings diff (we only compare keys we export/import).
    automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
    delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
    tt = get_terminal_transcript_settings(group.doc)
    default_send_to = get_default_send_to(group.doc)
    current_settings: Dict[str, Any] = {
        "default_send_to": default_send_to,
        "nudge_after_seconds": _as_int(automation.get("nudge_after_seconds", 300), 300),
        "reply_required_nudge_after_seconds": _as_int(automation.get("reply_required_nudge_after_seconds", 300), 300),
        "attention_ack_nudge_after_seconds": _as_int(automation.get("attention_ack_nudge_after_seconds", 600), 600),
        "unread_nudge_after_seconds": _as_int(automation.get("unread_nudge_after_seconds", 900), 900),
        "nudge_digest_min_interval_seconds": _as_int(automation.get("nudge_digest_min_interval_seconds", 120), 120),
        "nudge_max_repeats_per_obligation": _as_int(automation.get("nudge_max_repeats_per_obligation", 3), 3),
        "nudge_escalate_after_repeats": _as_int(automation.get("nudge_escalate_after_repeats", 2), 2),
        "auto_mark_on_delivery": coerce_bool(delivery.get("auto_mark_on_delivery"), default=False),
        "actor_idle_timeout_seconds": _as_int(automation.get("actor_idle_timeout_seconds", 600), 600),
        "keepalive_delay_seconds": _as_int(automation.get("keepalive_delay_seconds", 120), 120),
        "keepalive_max_per_actor": _as_int(automation.get("keepalive_max_per_actor", 3), 3),
        "silence_timeout_seconds": _as_int(automation.get("silence_timeout_seconds", 600), 600),
        "help_nudge_interval_seconds": _as_int(automation.get("help_nudge_interval_seconds", 600), 600),
        "help_nudge_min_messages": _as_int(automation.get("help_nudge_min_messages", 10), 10),
        "min_interval_seconds": _as_int(delivery.get("min_interval_seconds", 0), 0),
        "terminal_transcript_visibility": str(tt.get("visibility") or "foreman"),
        "terminal_transcript_notify_tail": coerce_bool(tt.get("notify_tail"), default=True),
        "terminal_transcript_notify_lines": _as_int(tt.get("notify_lines", 20), 20),
    }
    desired_settings = template.settings.model_dump()
    settings_changed: Dict[str, Tuple[Any, Any]] = {}
    for k, cur in current_settings.items():
        nxt = desired_settings.get(k, cur)
        if cur != nxt:
            settings_changed[k] = (cur, nxt)

    # Prompts diff summary (changed, current_len, new_len, current_source, new_source).
    # Source is "home" (group override exists under CCCC_HOME) or "builtin" (no override file).
    prompts_changed: Dict[str, Tuple[bool, int, int, str, str]] = {}
    for kind, filename, builtin in (
        ("preamble", PREAMBLE_FILENAME, DEFAULT_PREAMBLE_BODY),
        ("help", HELP_FILENAME, load_builtin_help_markdown()),
    ):
        pf = read_group_prompt_file(group, filename)
        cur_has_override = bool(pf.found and isinstance(pf.content, str) and pf.content.strip())
        cur_source = "home" if cur_has_override else "builtin"
        cur_body = str(pf.content or "") if cur_has_override else str(builtin or "")

        new_raw = getattr(template.prompts, kind, None)
        new_has_override = bool(new_raw is not None and str(new_raw).strip())
        new_source = "home" if new_has_override else "builtin"
        new_body = str(new_raw) if new_has_override else str(builtin or "")

        changed = cur_source != new_source
        if not changed and cur_source == "home" and new_source == "home":
            changed = cur_body != new_body

        prompts_changed[kind] = (changed, len(cur_body), len(new_body), cur_source, new_source)

    return GroupTemplateDiff(
        actors_add=add_ids,
        actors_update=update_ids,
        actors_remove=remove_ids,
        settings_changed=settings_changed,
        prompts_changed=prompts_changed,
    )
