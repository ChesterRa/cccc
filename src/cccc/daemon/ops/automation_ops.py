"""Automation operations for daemon."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ...contracts.v1 import AutomationRule, AutomationRuleSet, DaemonError, DaemonResponse
from ...kernel.actors import find_actor, get_effective_role
from ...kernel.group import default_automation_ruleset_doc, load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...util.conv import coerce_bool
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso
from ..automation import (
    _load_ruleset as load_automation_ruleset,
    automation_supported_vars,
    build_automation_status,
)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _parse_expected_version(args: Dict[str, Any]) -> tuple[Optional[int], Optional[DaemonResponse]]:
    expected_version_raw = args.get("expected_version")
    if expected_version_raw is None:
        return None, None
    try:
        return int(expected_version_raw), None
    except Exception:
        return None, _error("invalid_request", "expected_version must be an integer")


def _ensure_automation_doc(group: Any) -> Dict[str, Any]:
    raw_automation = group.doc.get("automation")
    seed = default_automation_ruleset_doc()
    if not isinstance(raw_automation, dict):
        automation = seed
    else:
        automation = dict(raw_automation)
        has_rules = isinstance(automation.get("rules"), list)
        has_snippets = isinstance(automation.get("snippets"), dict)
        if not has_rules and not has_snippets:
            automation["rules"] = seed.get("rules", [])
            automation["snippets"] = seed.get("snippets", {})
        else:
            if not has_rules:
                automation["rules"] = []
            if not has_snippets:
                automation["snippets"] = {}
    try:
        version = int(automation.get("version") or 0)
    except Exception:
        version = 0
    if version <= 0:
        version = 1
    automation["version"] = version
    if group.doc.get("automation") != automation:
        group.doc["automation"] = automation
        group.save()
    else:
        group.doc["automation"] = automation
    return automation


def _automation_version(group: Any) -> int:
    automation = _ensure_automation_doc(group)
    try:
        version = int(automation.get("version") or 0)
    except Exception:
        version = 0
    return max(1, version)


def _reconcile_automation_state_after_ruleset_change(
    group: Any,
    *,
    previous: AutomationRuleSet,
    current: AutomationRuleSet,
) -> None:
    state_path = group.path / "state" / "automation.json"
    raw = read_json(state_path)
    state = raw if isinstance(raw, dict) else {}
    rules_state = state.get("rules") if isinstance(state.get("rules"), dict) else {}
    if not isinstance(rules_state, dict):
        rules_state = {}
    changed = False

    prev_by_id: Dict[str, AutomationRule] = {}
    for rule in previous.rules:
        rid = str(rule.id or "").strip()
        if rid:
            prev_by_id[rid] = rule

    next_by_id: Dict[str, AutomationRule] = {}
    for rule in current.rules:
        rid = str(rule.id or "").strip()
        if rid:
            next_by_id[rid] = rule

    next_ids = set(next_by_id.keys())
    stale_ids = [rid for rid in list(rules_state.keys()) if rid not in next_ids]
    for rid in stale_ids:
        rules_state.pop(rid, None)
        changed = True

    for rid, next_rule in next_by_id.items():
        entry = rules_state.get(rid)
        if not isinstance(entry, dict):
            continue
        trigger_kind = str(getattr(next_rule.trigger, "kind", "") or "").strip()
        if trigger_kind != "at":
            if entry.pop("at_fired", None) is not None:
                changed = True
            slot_key = str(entry.get("last_slot_key") or "")
            if slot_key.startswith("at:"):
                entry.pop("last_slot_key", None)
                changed = True
            continue

        prev_rule = prev_by_id.get(rid)
        prev_at = ""
        if prev_rule is not None and str(getattr(prev_rule.trigger, "kind", "") or "").strip() == "at":
            prev_at = str(getattr(prev_rule.trigger, "at", "") or "").strip()
        next_at = str(getattr(next_rule.trigger, "at", "") or "").strip()
        if prev_at and next_at and prev_at == next_at:
            continue

        if entry.pop("at_fired", None) is not None:
            changed = True
        slot_key = str(entry.get("last_slot_key") or "")
        if slot_key.startswith("at:"):
            entry.pop("last_slot_key", None)
            changed = True

    if not changed:
        return

    state["rules"] = rules_state
    state["updated_at"] = utc_now_iso()
    atomic_write_json(state_path, state)


def _set_automation_ruleset(group: Any, *, ruleset: AutomationRuleSet) -> int:
    previous_ruleset = load_automation_ruleset(group)
    automation = _ensure_automation_doc(group)
    automation["rules"] = [r.model_dump(exclude_none=True) for r in (ruleset.rules or [])]
    automation["snippets"] = dict(ruleset.snippets or {})
    try:
        old_version = int(automation.get("version") or 0)
    except Exception:
        old_version = 0
    automation["version"] = max(1, old_version) + 1
    group.doc["automation"] = automation
    group.save()
    _reconcile_automation_state_after_ruleset_change(group, previous=previous_ruleset, current=ruleset)
    return int(automation["version"])


def _validate_automation_rule_action_trigger(rule: AutomationRule) -> None:
    rid = str(rule.id or "").strip() or "<unknown>"
    trigger_kind = str(getattr(rule.trigger, "kind", "") or "").strip()
    action_kind = str(getattr(rule.action, "kind", "notify") or "notify").strip()
    if action_kind in {"group_state", "actor_control"} and trigger_kind != "at":
        raise ValueError(f'rule "{rid}": action.kind={action_kind} only supports trigger.kind=at')


def _reject_legacy_automation_rule_shape(rule_raw: Dict[str, Any], *, loc: str) -> None:
    legacy_root: list[str] = []
    if "name" in rule_raw:
        legacy_root.append("name->id")
    if "actions" in rule_raw:
        legacy_root.append("actions->action")
    if "schedule" in rule_raw:
        legacy_root.append("schedule->trigger")
    if "every_minutes" in rule_raw:
        legacy_root.append("every_minutes->trigger.every_seconds")

    trigger = rule_raw.get("trigger")
    if isinstance(trigger, dict) and "every_minutes" in trigger:
        legacy_root.append("trigger.every_minutes->trigger.every_seconds")

    action = rule_raw.get("action")
    if isinstance(action, dict):
        if "type" in action:
            legacy_root.append("action.type->action.kind")
        if "message_template" in action:
            legacy_root.append("action.message_template->action.snippet_ref or action.message")

    if legacy_root:
        hints = ", ".join(legacy_root)
        raise ValueError(
            f"{loc}: legacy automation rule shape is not supported; use canonical fields "
            f"(id, trigger, action, rule_id, ruleset). Details: {hints}"
        )


def _actor_role_or_none(group: Any, actor_id: str) -> str:
    aid = str(actor_id or "").strip()
    if not aid:
        return ""
    if find_actor(group, aid) is None:
        return ""
    try:
        return str(get_effective_role(group, aid) or "").strip()
    except Exception:
        return ""


def handle_group_automation_update(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    raw = args.get("ruleset") if isinstance(args.get("ruleset"), dict) else {}
    expected_version, err = _parse_expected_version(args)
    if err is not None:
        return err
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    try:
        require_group_permission(group, by=by, action="group.settings_update")
        if isinstance(raw, dict):
            raw_rules = raw.get("rules")
            if isinstance(raw_rules, list):
                for i, raw_rule in enumerate(raw_rules):
                    if isinstance(raw_rule, dict):
                        _reject_legacy_automation_rule_shape(raw_rule, loc=f"ruleset.rules[{i}]")
        ruleset = AutomationRuleSet.model_validate(raw)
        for rule in ruleset.rules:
            _validate_automation_rule_action_trigger(rule)
        if by and by != "user":
            for rule in ruleset.rules:
                action = getattr(rule, "action", None)
                kind = str(getattr(action, "kind", "notify") or "notify").strip()
                if kind != "notify":
                    raise ValueError("agents can only manage notify automation rules")
        current_version = _automation_version(group)
        if expected_version is not None and expected_version != current_version:
            return _error(
                "version_conflict",
                "automation version mismatch",
                details={"expected_version": expected_version, "current_version": current_version},
            )
        next_version = _set_automation_ruleset(group, ruleset=ruleset)
    except Exception as e:
        return _error("group_automation_update_failed", str(e))

    ev = append_event(
        group.ledger_path,
        kind="group.automation_update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={
            "rules": [str(r.id) for r in (ruleset.rules or [])],
            "snippets": sorted([str(k) for k in (ruleset.snippets or {}).keys()]),
            "version": int(next_version),
        },
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "ruleset": ruleset.model_dump(), "version": int(next_version), "event": ev},
    )


def handle_group_automation_state(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    role = ""
    if by and by != "user":
        role = _actor_role_or_none(group, by)
        if not role:
            return _error("permission_denied", f"unknown actor: {by}")

    ruleset = load_automation_ruleset(group)
    status = build_automation_status(group)
    version = _automation_version(group)

    if role == "peer":
        visible_rules: list[Any] = []
        visible_ids: set[str] = set()
        for rule in ruleset.rules:
            rid = str(rule.id or "").strip()
            if not rid:
                continue
            scope = str(rule.scope or "group")
            owner = str(rule.owner_actor_id or "").strip()
            if scope == "group" or owner == by:
                visible_rules.append(rule)
                visible_ids.add(rid)
        snippet_refs: set[str] = set()
        for rule in visible_rules:
            action = getattr(rule, "action", None)
            if str(getattr(action, "kind", "notify") or "notify").strip() != "notify":
                continue
            ref = str(getattr(action, "snippet_ref", "") or "").strip()
            if ref:
                snippet_refs.add(ref)
        snippets = {k: v for k, v in (ruleset.snippets or {}).items() if k in snippet_refs}
        status = {rid: st for rid, st in status.items() if rid in visible_ids}
        ruleset_out = AutomationRuleSet(rules=visible_rules, snippets=snippets)
    else:
        ruleset_out = ruleset

    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "ruleset": ruleset_out.model_dump(),
            "status": status,
            "supported_vars": automation_supported_vars(),
            "version": int(version),
            "server_now": utc_now_iso(),
            "config_path": str(group.path / "group.yaml"),
        },
    )


def handle_group_automation_manage(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    actions: list[Any] = []
    actions_raw = args.get("actions")
    if isinstance(actions_raw, list):
        actions.extend(actions_raw)
    expected_version, err = _parse_expected_version(args)
    if err is not None:
        return err
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actions:
        return _error("invalid_request", "actions must be a non-empty array")
    for idx, action in enumerate(actions):
        if not isinstance(action, dict):
            return _error("invalid_request", f"action[{idx}] must be an object")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    caller_role = "user"
    caller_id = by
    if by and by != "user":
        role = _actor_role_or_none(group, by)
        if role not in ("foreman", "peer"):
            return _error("permission_denied", f"unknown actor: {by}")
        caller_role = role

    current_ruleset = load_automation_ruleset(group)
    current_version = _automation_version(group)
    if expected_version is not None and expected_version != current_version:
        return _error(
            "version_conflict",
            "automation version mismatch",
            details={"expected_version": expected_version, "current_version": current_version},
        )

    rules_order: list[str] = []
    rules_by_id: Dict[str, Any] = {}
    for rule in current_ruleset.rules:
        rid = str(rule.id or "").strip()
        if not rid or rid in rules_by_id:
            continue
        rules_order.append(rid)
        rules_by_id[rid] = rule
    snippets: Dict[str, str] = dict(current_ruleset.snippets or {})

    def _enforce_peer_rule(rule: Any, *, existing: Optional[Any] = None) -> Any:
        if caller_role != "peer":
            return rule
        if existing is not None:
            existing_scope = str(existing.scope or "group")
            existing_owner = str(existing.owner_actor_id or "").strip()
            if existing_scope != "personal" or existing_owner != caller_id:
                raise ValueError("peer can only manage own personal rules")
        scope = str(rule.scope or "group")
        owner = str(rule.owner_actor_id or "").strip()
        if scope != "personal":
            raise ValueError("peer rules must use scope=personal")
        if owner != caller_id:
            raise ValueError("peer rules must set owner_actor_id to self")
        to = [str(x).strip() for x in (rule.to or []) if isinstance(x, str) and str(x).strip()]
        if len(to) != 1 or to[0] != caller_id:
            raise ValueError("peer rules must target only self actor_id")
        return rule

    def _enforce_actor_action_kind(rule: Any) -> Any:
        if caller_role == "user":
            return rule
        action = getattr(rule, "action", None)
        kind = str(getattr(action, "kind", "notify") or "notify").strip()
        if kind != "notify":
            raise ValueError("agents can only manage notify automation rules")
        return rule

    def _normalize_rule(rule: Any) -> Any:
        scope = str(rule.scope or "group")
        owner = str(rule.owner_actor_id or "").strip()
        if scope == "group":
            if owner:
                rule = rule.model_copy(update={"owner_actor_id": None})
        elif scope == "personal":
            if not owner:
                raise ValueError("personal rule requires owner_actor_id")
        else:
            raise ValueError(f"invalid scope: {scope}")
        return rule

    applied_actions: List[Dict[str, Any]] = []

    try:
        for idx, raw_action in enumerate(actions):
            if not isinstance(raw_action, dict):
                raise ValueError(f"action[{idx}] must be an object")
            action_type = str(raw_action.get("type") or "").strip()
            if not action_type:
                raise ValueError(f"action[{idx}].type is required")

            if action_type == "create_rule":
                rule_raw = raw_action.get("rule")
                if not isinstance(rule_raw, dict):
                    raise ValueError("create_rule requires rule")
                _reject_legacy_automation_rule_shape(rule_raw, loc="action.create_rule.rule")
                rule = AutomationRule.model_validate(rule_raw)
                rid = str(rule.id or "").strip()
                if not rid:
                    raise ValueError("rule.id is required")
                if rid in rules_by_id:
                    raise ValueError(f"rule already exists: {rid}")
                rule = _normalize_rule(rule)
                rule = _enforce_peer_rule(rule)
                rule = _enforce_actor_action_kind(rule)
                _validate_automation_rule_action_trigger(rule)
                rules_by_id[rid] = rule
                rules_order.append(rid)
                applied_actions.append({"type": action_type, "rule_id": rid})
                continue

            if action_type == "update_rule":
                rule_raw = raw_action.get("rule")
                if not isinstance(rule_raw, dict):
                    raise ValueError("update_rule requires rule")
                _reject_legacy_automation_rule_shape(rule_raw, loc="action.update_rule.rule")
                rule = AutomationRule.model_validate(rule_raw)
                rid = str(rule.id or "").strip()
                if not rid:
                    raise ValueError("rule.id is required")
                existing = rules_by_id.get(rid)
                if existing is None:
                    raise ValueError(f"rule not found: {rid}")
                _enforce_peer_rule(existing)
                _enforce_actor_action_kind(existing)
                rule = _normalize_rule(rule)
                rule = _enforce_peer_rule(rule, existing=existing)
                rule = _enforce_actor_action_kind(rule)
                _validate_automation_rule_action_trigger(rule)
                rules_by_id[rid] = rule
                applied_actions.append({"type": action_type, "rule_id": rid})
                continue

            if action_type == "set_rule_enabled":
                rid = str(raw_action.get("rule_id") or "").strip()
                if not rid:
                    raise ValueError("set_rule_enabled requires rule_id")
                existing = rules_by_id.get(rid)
                if existing is None:
                    raise ValueError(f"rule not found: {rid}")
                _enforce_peer_rule(existing)
                _enforce_actor_action_kind(existing)
                enabled = coerce_bool(raw_action.get("enabled"), default=False)
                rules_by_id[rid] = existing.model_copy(update={"enabled": bool(enabled)})
                applied_actions.append({"type": action_type, "rule_id": rid, "enabled": bool(enabled)})
                continue

            if action_type == "delete_rule":
                rid = str(raw_action.get("rule_id") or "").strip()
                if not rid:
                    raise ValueError("delete_rule requires rule_id")
                existing = rules_by_id.get(rid)
                if existing is None:
                    raise ValueError(f"rule not found: {rid}")
                _enforce_peer_rule(existing)
                _enforce_actor_action_kind(existing)
                rules_by_id.pop(rid, None)
                rules_order = [x for x in rules_order if x != rid]
                applied_actions.append({"type": action_type, "rule_id": rid})
                continue

            if action_type == "replace_all_rules":
                if caller_role not in ("user", "foreman"):
                    raise ValueError("replace_all_rules is foreman-only")
                ruleset_raw = raw_action.get("ruleset")
                if not isinstance(ruleset_raw, dict):
                    raise ValueError("replace_all_rules requires ruleset")
                raw_rules = ruleset_raw.get("rules")
                if isinstance(raw_rules, list):
                    for i, raw_rule in enumerate(raw_rules):
                        if isinstance(raw_rule, dict):
                            _reject_legacy_automation_rule_shape(
                                raw_rule,
                                loc=f"action.replace_all_rules.ruleset.rules[{i}]",
                            )
                replacement = AutomationRuleSet.model_validate(ruleset_raw)
                seen: set[str] = set()
                new_order: list[str] = []
                new_map: Dict[str, Any] = {}
                for rule in replacement.rules:
                    rid = str(rule.id or "").strip()
                    if not rid:
                        raise ValueError("rule.id is required")
                    if rid in seen:
                        raise ValueError(f"duplicate rule id: {rid}")
                    seen.add(rid)
                    normalized = _normalize_rule(rule)
                    normalized = _enforce_actor_action_kind(normalized)
                    _validate_automation_rule_action_trigger(normalized)
                    new_order.append(rid)
                    new_map[rid] = normalized
                rules_order = new_order
                rules_by_id = new_map
                snippets = dict(replacement.snippets or {})
                applied_actions.append({"type": action_type, "rules": len(rules_order), "snippets": len(snippets)})
                continue

            raise ValueError(f"unsupported action type: {action_type}")
    except Exception as e:
        return _error("group_automation_manage_failed", str(e))

    next_rules = [rules_by_id[rid] for rid in rules_order if rid in rules_by_id]
    next_ruleset = AutomationRuleSet(rules=next_rules, snippets=snippets)

    changed = next_ruleset.model_dump() != current_ruleset.model_dump()
    if changed:
        next_version = _set_automation_ruleset(group, ruleset=next_ruleset)
        ev = append_event(
            group.ledger_path,
            kind="group.automation_update",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "rules": [str(r.id) for r in next_rules],
                "snippets": sorted([str(k) for k in snippets.keys()]),
                "version": int(next_version),
                "actions": applied_actions,
            },
        )
    else:
        next_version = current_version
        ev = None

    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "ruleset": next_ruleset.model_dump(),
            "status": build_automation_status(group),
            "supported_vars": automation_supported_vars(),
            "version": int(next_version),
            "server_now": utc_now_iso(),
            "applied_actions": applied_actions,
            "changed": bool(changed),
            "event": ev,
        },
    )


def handle_group_automation_reset_baseline(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    expected_version, err = _parse_expected_version(args)
    if err is not None:
        return err
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    try:
        require_group_permission(group, by=by, action="group.settings_update")
        current_version = _automation_version(group)
        if expected_version is not None and expected_version != current_version:
            return _error(
                "version_conflict",
                "automation version mismatch",
                details={"expected_version": expected_version, "current_version": current_version},
            )
        seed = default_automation_ruleset_doc()
        baseline = AutomationRuleSet.model_validate(
            {
                "rules": list(seed.get("rules", [])),
                "snippets": dict(seed.get("snippets", {})),
            }
        )
        next_version = _set_automation_ruleset(group, ruleset=baseline)
    except Exception as e:
        return _error("group_automation_reset_baseline_failed", str(e))

    ev = append_event(
        group.ledger_path,
        kind="group.automation_update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={
            "rules": [str(r.id) for r in baseline.rules],
            "snippets": sorted([str(k) for k in baseline.snippets.keys()]),
            "version": int(next_version),
            "source": "baseline_reset",
        },
    )
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "ruleset": baseline.model_dump(),
            "status": build_automation_status(group),
            "supported_vars": automation_supported_vars(),
            "version": int(next_version),
            "server_now": utc_now_iso(),
            "config_path": str(group.path / "group.yaml"),
            "event": ev,
        },
    )


def try_handle_group_automation_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "group_automation_update":
        return handle_group_automation_update(args)
    if op == "group_automation_state":
        return handle_group_automation_state(args)
    if op == "group_automation_manage":
        return handle_group_automation_manage(args)
    if op == "group_automation_reset_baseline":
        return handle_group_automation_reset_baseline(args)
    return None
