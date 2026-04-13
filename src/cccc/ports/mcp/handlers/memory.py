from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Type


def _handle_memory_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_caller_from_by: Callable[[Dict[str, Any]], str],
    coerce_bool: Callable[..., bool],
    call_daemon_or_raise: Callable[..., Dict[str, Any]],
    mcp_error_cls: Type[Exception],
    build_memory_guide: Callable[[str], Dict[str, str]],
) -> Optional[Dict[str, Any]]:
    # Keep build_memory_guide injectable for bootstrap/help payloads, but cccc_memory
    # no longer exposes guide action in hard-cut ReMe mode.
    _ = build_memory_guide
    if name == "cccc_memory":
        action = str(arguments.get("action") or "search").strip().lower()
        gid = resolve_group_id(arguments)

        if action == "layout_get":
            return call_daemon_or_raise({"op": "memory_reme_layout_get", "args": {"group_id": gid}})

        if action == "search":
            args: Dict[str, Any] = {"group_id": gid}
            for field in ("query", "max_results", "min_score", "vector_weight", "candidate_multiplier"):
                val = arguments.get(field)
                if val is not None:
                    args[field] = val
            if "sources" in arguments:
                args["sources"] = arguments["sources"]
            return call_daemon_or_raise({"op": "memory_reme_search", "args": args})

        if action == "get":
            path = str(arguments.get("path") or "").strip()
            if not path:
                raise mcp_error_cls("validation_error", "missing path")
            args = {"group_id": gid, "path": path}
            if "offset" in arguments:
                args["offset"] = arguments["offset"]
            if "limit" in arguments:
                args["limit"] = arguments["limit"]
            return call_daemon_or_raise({"op": "memory_reme_get", "args": args})

        if action == "write":
            target = str(arguments.get("target") or "").strip().lower()
            content = str(arguments.get("content") or "")
            if target not in {"memory", "daily"}:
                raise mcp_error_cls("validation_error", "target must be one of: memory, daily")
            if not content.strip():
                raise mcp_error_cls("validation_error", "missing content")
            args = {"group_id": gid, "target": target, "content": content}
            for field in ("date", "mode", "idempotency_key", "actor_id", "dedup_intent", "dedup_query"):
                val = arguments.get(field)
                if val is not None:
                    args[field] = val
            for field in ("source_refs", "tags", "supersedes"):
                val = arguments.get(field)
                if isinstance(val, list):
                    args[field] = val
            return call_daemon_or_raise({"op": "memory_reme_write", "args": args})

        if action == "promote_experience":
            candidate_id = str(arguments.get("candidate_id") or "").strip()
            if not candidate_id:
                raise mcp_error_cls("validation_error", "missing candidate_id")
            by = resolve_caller_from_by(arguments)
            args = {
                "group_id": gid,
                "candidate_id": candidate_id,
                "by": by,
                "dry_run": coerce_bool(arguments.get("dry_run"), default=False),
            }
            promote_result = call_daemon_or_raise({"op": "experience_promote_to_memory", "args": args})
            if not isinstance(promote_result, dict):
                return promote_result
            if bool(args.get("dry_run")):
                return promote_result
            commit_state = str(promote_result.get("commit_state") or "")
            asset_write = promote_result.get("asset_write") if isinstance(promote_result.get("asset_write"), dict) else {}
            if commit_state == "disk_committed" and str(asset_write.get("status") or "") == "written":
                delivery_result = call_daemon_or_raise(
                    {
                        "op": "experience_runtime_prompt_delivery",
                        "args": {
                            "group_id": gid,
                            "candidate_id": candidate_id,
                        },
                    }
                )
                if isinstance(delivery_result, dict) and isinstance(delivery_result.get("runtime_prompt_delivery"), dict):
                    merged = dict(promote_result)
                    merged["runtime_prompt_delivery"] = delivery_result["runtime_prompt_delivery"]
                    return merged
            return promote_result

        if action == "govern_experience":
            lifecycle_action = str(arguments.get("lifecycle_action") or "").strip().lower()
            if lifecycle_action not in {"reject", "merge", "supersede"}:
                raise mcp_error_cls("validation_error", "lifecycle_action must be one of: reject, merge, supersede")
            by = resolve_caller_from_by(arguments)
            args = {
                "group_id": gid,
                "by": by,
                "lifecycle_action": lifecycle_action,
                "dry_run": coerce_bool(arguments.get("dry_run"), default=False),
            }
            for field in ("candidate_id", "target_candidate_id", "reason"):
                val = arguments.get(field)
                if val is not None:
                    args[field] = val
            source_candidate_ids = arguments.get("source_candidate_ids")
            if isinstance(source_candidate_ids, list):
                args["source_candidate_ids"] = source_candidate_ids
            return call_daemon_or_raise({"op": "experience_govern", "args": args})

        if action == "repair_experience":
            candidate_id = str(arguments.get("candidate_id") or "").strip()
            if not candidate_id:
                raise mcp_error_cls("validation_error", "missing candidate_id")
            by = resolve_caller_from_by(arguments)
            args = {
                "group_id": gid,
                "candidate_id": candidate_id,
                "by": by,
                "dry_run": coerce_bool(arguments.get("dry_run"), default=False),
            }
            return call_daemon_or_raise({"op": "experience_repair_memory", "args": args})

        if action == "report_skill_usage":
            skill_id = str(arguments.get("skill_id") or "").strip()
            if not skill_id:
                raise mcp_error_cls("validation_error", "missing skill_id")
            by = resolve_caller_from_by(arguments)
            turn_id = str(arguments.get("turn_id") or "").strip()
            evidence_type = str(arguments.get("evidence_type") or "").strip()
            if not turn_id:
                raise mcp_error_cls("validation_error", "missing turn_id")
            if not evidence_type:
                raise mcp_error_cls("validation_error", "missing evidence_type")
            args = {
                "group_id": gid,
                "skill_id": skill_id,
                "by": by,
                "actor_id": arguments.get("actor_id") or by,
                "turn_id": turn_id,
                "evidence_type": evidence_type,
                "outcome": arguments.get("outcome") or "",
                "reason": arguments.get("reason") or "",
                "generate_patch": coerce_bool(arguments.get("generate_patch"), default=False),
            }
            if isinstance(arguments.get("evidence_payload"), dict):
                args["evidence_payload"] = arguments["evidence_payload"]
            if arguments.get("patch_kind") is not None:
                args["patch_kind"] = arguments.get("patch_kind")
            if isinstance(arguments.get("proposed_delta"), dict):
                args["proposed_delta"] = arguments["proposed_delta"]
            return call_daemon_or_raise({"op": "procedural_skill_report_usage", "args": args})

        if action == "govern_skill_patch":
            candidate_id = str(arguments.get("candidate_id") or "").strip()
            lifecycle_action = str(arguments.get("lifecycle_action") or "").strip().lower()
            if not candidate_id:
                raise mcp_error_cls("validation_error", "missing candidate_id")
            if lifecycle_action not in {"merge", "reject"}:
                raise mcp_error_cls("validation_error", "lifecycle_action must be one of: merge, reject")
            by = resolve_caller_from_by(arguments)
            args = {
                "group_id": gid,
                "candidate_id": candidate_id,
                "lifecycle_action": lifecycle_action,
                "by": by,
                "reason": arguments.get("reason") or "",
            }
            return call_daemon_or_raise({"op": "procedural_skill_govern_patch", "args": args})

        raise mcp_error_cls(
            "invalid_request",
            "cccc_memory action must be one of: layout_get/search/get/write/promote_experience/govern_experience/repair_experience/report_skill_usage/govern_skill_patch",
        )

    if name == "cccc_memory_admin":
        gid = resolve_group_id(arguments)
        action = str(arguments.get("action") or "index_sync").strip().lower()

        if action == "index_sync":
            args = {"group_id": gid, "mode": str(arguments.get("mode") or "scan")}
            return call_daemon_or_raise({"op": "memory_reme_index_sync", "args": args})

        if action == "context_check":
            raw_messages = arguments.get("messages")
            if not isinstance(raw_messages, list):
                raise mcp_error_cls("validation_error", "messages must be an array")
            args: Dict[str, Any] = {"group_id": gid, "messages": raw_messages}
            for field in ("context_window_tokens", "reserve_tokens", "keep_recent_tokens"):
                val = arguments.get(field)
                if val is not None:
                    args[field] = val
            return call_daemon_or_raise({"op": "memory_reme_context_check", "args": args})

        if action == "compact":
            msgs = arguments.get("messages_to_summarize")
            if not isinstance(msgs, list):
                raise mcp_error_cls("validation_error", "messages_to_summarize must be an array")
            args = {
                "group_id": gid,
                "messages_to_summarize": msgs,
                "return_prompt": coerce_bool(arguments.get("return_prompt"), default=False),
            }
            turn_prefix = arguments.get("turn_prefix_messages")
            if isinstance(turn_prefix, list):
                args["turn_prefix_messages"] = turn_prefix
            previous_summary = arguments.get("previous_summary")
            if previous_summary is not None:
                args["previous_summary"] = previous_summary
            language = arguments.get("language")
            if language is not None:
                args["language"] = language
            return call_daemon_or_raise({"op": "memory_reme_compact", "args": args})

        if action == "daily_flush":
            msgs = arguments.get("messages")
            if not isinstance(msgs, list):
                raise mcp_error_cls("validation_error", "messages must be an array")
            args: Dict[str, Any] = {
                "group_id": gid,
                "messages": msgs,
                "return_prompt": coerce_bool(arguments.get("return_prompt"), default=False),
            }
            for field in ("date", "version", "language", "actor_id", "signal_pack_token_budget", "dedup_intent", "dedup_query"):
                val = arguments.get(field)
                if val is not None:
                    args[field] = val
            signal_pack = arguments.get("signal_pack")
            if isinstance(signal_pack, dict):
                args["signal_pack"] = signal_pack
            return call_daemon_or_raise({"op": "memory_reme_daily_flush", "args": args})

        raise mcp_error_cls(
            "invalid_request",
            "cccc_memory_admin action must be one of: index_sync/context_check/compact/daily_flush",
        )

    return None
