"""MCP handler functions for automation tools."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ..common import MCPError, _call_daemon_or_raise


def automation_state(*, group_id: str, by: str) -> Dict[str, Any]:
    """Read automation reminders/status visible to caller."""
    return _call_daemon_or_raise({
        "op": "group_automation_state",
        "args": {"group_id": group_id, "by": by},
    })


def automation_manage(
    *,
    group_id: str,
    by: str,
    actions: List[Dict[str, Any]],
    expected_version: Optional[int] = None,
) -> Dict[str, Any]:
    """Manage automation reminders incrementally."""
    req_args: Dict[str, Any] = {
        "group_id": group_id,
        "by": by,
        "actions": actions,
    }
    if expected_version is not None:
        req_args["expected_version"] = int(expected_version)
    return _call_daemon_or_raise({"op": "group_automation_manage", "args": req_args})


def _assert_agent_notify_only_actions(actions: List[Dict[str, Any]]) -> None:
    for idx, action in enumerate(actions):
        action_type = str(action.get("type") or "").strip()
        if action_type in {"create_rule", "update_rule"}:
            rule = action.get("rule")
            if not isinstance(rule, dict):
                continue
            action_doc = rule.get("action")
            if not isinstance(action_doc, dict):
                continue
            kind = str(action_doc.get("kind") or "notify").strip()
            if kind != "notify":
                raise MCPError(
                    code="permission_denied",
                    message=f"actions[{idx}] uses action.kind={kind}; agents may only manage notify rules",
                )
            continue
        if action_type == "replace_all_rules":
            ruleset = action.get("ruleset")
            if not isinstance(ruleset, dict):
                continue
            rules = ruleset.get("rules")
            if not isinstance(rules, list):
                continue
            for j, rule in enumerate(rules):
                if not isinstance(rule, dict):
                    continue
                action_doc = rule.get("action")
                if not isinstance(action_doc, dict):
                    continue
                kind = str(action_doc.get("kind") or "notify").strip()
                if kind != "notify":
                    raise MCPError(
                        code="permission_denied",
                        message=f"actions[{idx}].rules[{j}] uses action.kind={kind}; agents may only manage notify rules",
                    )


def _assert_action_trigger_compat(actions: List[Dict[str, Any]]) -> None:
    def _validate_rule(rule: Dict[str, Any], *, loc: str) -> None:
        action_doc = rule.get("action")
        trigger_doc = rule.get("trigger")
        if not isinstance(action_doc, dict) or not isinstance(trigger_doc, dict):
            return
        action_kind = str(action_doc.get("kind") or "notify").strip()
        trigger_kind = str(trigger_doc.get("kind") or "").strip()
        if action_kind in {"group_state", "actor_control"} and trigger_kind != "at":
            raise MCPError(
                code="invalid_request",
                message=f"{loc} uses action.kind={action_kind}; only one-time trigger.kind=at is allowed",
            )

    for idx, action in enumerate(actions):
        action_type = str(action.get("type") or "").strip()
        if action_type in {"create_rule", "update_rule"}:
            rule = action.get("rule")
            if isinstance(rule, dict):
                _validate_rule(rule, loc=f"actions[{idx}].rule")
            continue
        if action_type == "replace_all_rules":
            ruleset = action.get("ruleset")
            if not isinstance(ruleset, dict):
                continue
            rules = ruleset.get("rules")
            if not isinstance(rules, list):
                continue
            for j, rule in enumerate(rules):
                if isinstance(rule, dict):
                    _validate_rule(rule, loc=f"actions[{idx}].rules[{j}]")


def _map_simple_automation_op_to_action(arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    op = str(arguments.get("op") or "").strip().lower()
    if not op:
        return None
    if op == "create":
        rule = arguments.get("rule")
        if not isinstance(rule, dict):
            raise MCPError(code="invalid_request", message="op=create requires reminder object (rule)")
        return {"type": "create_rule", "rule": rule}
    if op == "update":
        rule = arguments.get("rule")
        if not isinstance(rule, dict):
            raise MCPError(code="invalid_request", message="op=update requires reminder object (rule)")
        return {"type": "update_rule", "rule": rule}
    if op == "enable":
        rule_id = str(arguments.get("rule_id") or "").strip()
        if not rule_id:
            raise MCPError(code="invalid_request", message="op=enable requires rule_id")
        return {"type": "set_rule_enabled", "rule_id": rule_id, "enabled": True}
    if op == "disable":
        rule_id = str(arguments.get("rule_id") or "").strip()
        if not rule_id:
            raise MCPError(code="invalid_request", message="op=disable requires rule_id")
        return {"type": "set_rule_enabled", "rule_id": rule_id, "enabled": False}
    if op == "delete":
        rule_id = str(arguments.get("rule_id") or "").strip()
        if not rule_id:
            raise MCPError(code="invalid_request", message="op=delete requires rule_id")
        return {"type": "delete_rule", "rule_id": rule_id}
    if op == "replace_all":
        ruleset = arguments.get("ruleset")
        if not isinstance(ruleset, dict):
            raise MCPError(code="invalid_request", message="op=replace_all requires reminder set object (ruleset)")
        return {"type": "replace_all_rules", "ruleset": ruleset}
    raise MCPError(
        code="invalid_request",
        message="op must be one of: create, update, enable, disable, delete, replace_all",
    )
