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
    if name == "cccc_memory_guide":
        topic = str(arguments.get("topic") or "").strip()
        if not topic:
            raise mcp_error_cls("validation_error", "missing topic")
        try:
            return build_memory_guide(topic)
        except ValueError as e:
            raise mcp_error_cls("validation_error", str(e))

    if name == "cccc_memory_store":
        gid = resolve_group_id(arguments)
        args: Dict[str, Any] = {"group_id": gid}
        for field in (
            "id",
            "content",
            "kind",
            "status",
            "confidence",
            "source_type",
            "source_ref",
            "scope_key",
            "actor_id",
            "task_id",
            "milestone_id",
            "event_ts",
            "strategy",
            "summary",
        ):
            val = arguments.get(field)
            if val is not None:
                args[field] = val
        if "tags" in arguments:
            args["tags"] = arguments["tags"]
        if arguments.get("solidify"):
            args["solidify"] = True
        return call_daemon_or_raise({"op": "memory_store", "args": args})

    if name == "cccc_memory_search":
        gid = resolve_group_id(arguments)
        args = {"group_id": gid}
        for field in ("query", "status", "kind", "actor_id", "task_id", "milestone_id", "confidence", "since", "until"):
            val = arguments.get(field)
            if val is not None:
                args[field] = val
        if "tags" in arguments:
            args["tags"] = arguments["tags"]
        if "limit" in arguments:
            args["limit"] = arguments["limit"]
        if "track_hit" in arguments:
            args["track_hit"] = coerce_bool(arguments.get("track_hit"), default=False)
        depth = arguments.get("depth")
        if depth is not None:
            args["depth"] = str(depth)
        return call_daemon_or_raise({"op": "memory_search", "args": args})

    if name == "cccc_memory_ingest":
        gid = resolve_group_id(arguments)
        args = {"group_id": gid}
        for field in ("mode", "limit", "actor_id"):
            val = arguments.get(field)
            if val is not None:
                args[field] = val
        if arguments.get("reset_watermark"):
            args["reset_watermark"] = True
        return call_daemon_or_raise({"op": "memory_ingest", "args": args})

    if name == "cccc_memory_stats":
        gid = resolve_group_id(arguments)
        return call_daemon_or_raise({"op": "memory_stats", "args": {"group_id": gid}})

    if name == "cccc_memory_export":
        gid = resolve_group_id(arguments)
        args = {"group_id": gid}
        if arguments.get("include_draft"):
            args["include_draft"] = True
        output_dir = arguments.get("output_dir")
        if output_dir:
            args["output_dir"] = str(output_dir)
        return call_daemon_or_raise({"op": "memory_export", "args": args})

    if name == "cccc_memory_delete":
        gid = resolve_group_id(arguments)
        args: Dict[str, Any] = {"group_id": gid}
        memory_id = str(arguments.get("id") or "").strip()
        if memory_id:
            args["id"] = memory_id

        raw_ids = arguments.get("ids")
        if raw_ids is not None:
            if not isinstance(raw_ids, list):
                raise mcp_error_cls("validation_error", "ids must be an array of strings")
            args["ids"] = [str(x) for x in raw_ids]

        if "id" not in args and "ids" not in args:
            raise mcp_error_cls("missing_id", "missing memory id or ids")

        return call_daemon_or_raise({"op": "memory_delete", "args": args})

    if name == "cccc_memory_decay":
        gid = resolve_group_id(arguments)
        args: Dict[str, Any] = {"group_id": gid}
        for field in ("draft_days", "zero_hit_days", "solid_review_days", "solid_max_hit", "limit"):
            val = arguments.get(field)
            if val is not None:
                args[field] = val
        return call_daemon_or_raise({"op": "memory_decay", "args": args})

    return None

