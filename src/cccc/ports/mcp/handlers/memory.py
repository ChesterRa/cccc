from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Type


def _handle_memory_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
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

        raise mcp_error_cls(
            "invalid_request",
            "cccc_memory action must be one of: layout_get/search/get/write",
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
