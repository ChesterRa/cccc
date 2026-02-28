from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, Optional

from fastapi import FastAPI, File, Form, HTTPException, Request, UploadFile
from fastapi.responses import StreamingResponse

from ....contracts.v1.automation import AutomationRuleSet
from ....kernel.blobs import resolve_blob_attachment_path
from ....kernel.group import load_group
from ....kernel.group_template import parse_group_template
from ....kernel.ledger import read_last_lines
from ....kernel.prompt_files import (
    DEFAULT_PREAMBLE_BODY,
    HELP_FILENAME,
    PREAMBLE_FILENAME,
    delete_group_prompt_file,
    load_builtin_help_markdown,
    read_group_prompt_file,
    write_group_prompt_file,
)
from ....util.conv import coerce_bool
from ....util.fs import atomic_write_text
from ..schemas import (
    AttachRequest,
    CreateGroupRequest,
    GroupAutomationManageRequest,
    GroupAutomationRequest,
    GroupAutomationResetBaselineRequest,
    GroupSettingsRequest,
    GroupTemplatePreviewRequest,
    GroupUpdateRequest,
    ProjectMdUpdateRequest,
    RepoPromptUpdateRequest,
    RouteContext,
    WEB_MAX_TEMPLATE_BYTES,
    _safe_int,
)


def register_group_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    @app.get("/api/v1/groups")
    async def groups() -> Dict[str, Any]:
        async def _fetch() -> Dict[str, Any]:
            return await ctx.daemon({"op": "groups"})

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        return await ctx.cached_json("groups", ttl, _fetch)

    @app.post("/api/v1/groups")
    async def group_create(req: CreateGroupRequest) -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_create", "args": {"title": req.title, "topic": req.topic, "by": req.by}})

    @app.post("/api/v1/groups/from_template")
    async def group_create_from_template(
        path: str = Form(...),
        title: str = Form("working-group"),
        topic: str = Form(""),
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "template_too_large", "message": "template too large"})
        template_text = raw.decode("utf-8", errors="replace")
        return await ctx.daemon(
            {
                "op": "group_create_from_template",
                "args": {"path": path, "title": title, "topic": topic, "by": by, "template": template_text},
            }
        )

    @app.post("/api/v1/templates/preview")
    async def template_preview(file: UploadFile = File(...)) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            return {"ok": False, "error": {"code": "template_too_large", "message": "template too large"}}
        template_text = raw.decode("utf-8", errors="replace")
        try:
            tpl = parse_group_template(template_text)
        except Exception as e:
            return {"ok": False, "error": {"code": "invalid_template", "message": str(e)}}

        def _prompt_preview(value: Any, limit: int = 2000) -> Dict[str, Any]:
            if value is None:
                return {"source": "builtin"}
            raw_text = str(value)
            if not raw_text.strip():
                return {"source": "builtin"}
            out = raw_text.strip()
            if len(out) > limit:
                out = out[:limit] + "\n…"
            # Templates now map prompt overrides to CCCC_HOME/group prompts.
            return {"source": "home", "chars": len(raw_text), "preview": out}

        return {
            "ok": True,
            "result": {
                "template": {
                    "kind": tpl.kind,
                    "v": tpl.v,
                    "title": tpl.title,
                    "topic": tpl.topic,
                    "exported_at": tpl.exported_at,
                    "cccc_version": tpl.cccc_version,
                    "actors": [
                        {
                            "id": a.actor_id,
                            "title": a.title,
                            "runtime": a.runtime,
                            "runner": a.runner,
                            "command": a.command,
                            "submit": a.submit,
                            "enabled": bool(a.enabled),
                        }
                        for a in tpl.actors
                    ],
                    "settings": tpl.settings.model_dump(),
                    "automation": {
                        "rules": len(tpl.automation.rules),
                        "snippets": len(tpl.automation.snippets),
                    },
                    "prompts": {
                        "preamble": _prompt_preview(tpl.prompts.preamble),
                        "help": _prompt_preview(tpl.prompts.help),
                    },
                }
            },
        }

    @app.get("/api/v1/groups/{group_id}")
    async def group_show(group_id: str) -> Dict[str, Any]:
        gid = str(group_id or "").strip()

        async def _fetch() -> Dict[str, Any]:
            return await ctx.daemon({"op": "group_show", "args": {"group_id": gid}})

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        return await ctx.cached_json(f"group:{gid}", ttl, _fetch)

    @app.put("/api/v1/groups/{group_id}")
    async def group_update(group_id: str, req: GroupUpdateRequest) -> Dict[str, Any]:
        """Update group metadata (title/topic)."""
        patch: Dict[str, Any] = {}
        if req.title is not None:
            patch["title"] = req.title
        if req.topic is not None:
            patch["topic"] = req.topic
        if not patch:
            return {"ok": True, "result": {"message": "no changes"}}
        return await ctx.daemon({"op": "group_update", "args": {"group_id": group_id, "by": req.by, "patch": patch}})

    @app.delete("/api/v1/groups/{group_id}")
    async def group_delete(group_id: str, confirm: str = "", by: str = "user") -> Dict[str, Any]:
        """Delete a group (requires confirm=group_id)."""
        if confirm != group_id:
            raise HTTPException(
                status_code=400,
                detail={"code": "confirmation_required", "message": f"confirm must equal group_id: {group_id}"}
            )
        return await ctx.daemon({"op": "group_delete", "args": {"group_id": group_id, "by": by}})

    @app.get("/api/v1/groups/{group_id}/context")
    async def group_context(group_id: str) -> Dict[str, Any]:
        """Get full group context (vision/overview/tasks/presence)."""
        gid = str(group_id or "").strip()

        async def _fetch() -> Dict[str, Any]:
            return await ctx.daemon({"op": "context_get", "args": {"group_id": gid}})

        ttl = max(0.0, min(5.0, ctx.exhibit_cache_ttl_s))
        return await ctx.cached_json(f"context:{gid}", ttl, _fetch)

    @app.get("/api/v1/groups/{group_id}/template/export")
    async def group_template_export(group_id: str) -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_template_export", "args": {"group_id": group_id}})

    @app.post("/api/v1/groups/{group_id}/template/preview")
    async def group_template_preview(group_id: str, req: GroupTemplatePreviewRequest) -> Dict[str, Any]:
        return await ctx.daemon({"op": "group_template_preview", "args": {"group_id": group_id, "template": req.template, "by": req.by}})

    @app.post("/api/v1/groups/{group_id}/template/preview_upload")
    async def group_template_preview_upload(
        group_id: str,
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "template_too_large", "message": "template too large"})
        template_text = raw.decode("utf-8", errors="replace")
        return await ctx.daemon({"op": "group_template_preview", "args": {"group_id": group_id, "template": template_text, "by": by}})

    @app.post("/api/v1/groups/{group_id}/template/import_replace")
    async def group_template_import_replace(
        group_id: str,
        confirm: str = Form(""),
        by: str = Form("user"),
        file: UploadFile = File(...),
    ) -> Dict[str, Any]:
        raw = await file.read()
        if len(raw) > WEB_MAX_TEMPLATE_BYTES:
            raise HTTPException(status_code=413, detail={"code": "template_too_large", "message": "template too large"})
        template_text = raw.decode("utf-8", errors="replace")
        return await ctx.daemon(
            {
                "op": "group_template_import_replace",
                "args": {"group_id": group_id, "confirm": confirm, "by": by, "template": template_text},
            }
        )

    @app.get("/api/v1/groups/{group_id}/tasks")
    async def group_tasks(group_id: str, task_id: Optional[str] = None) -> Dict[str, Any]:
        """List tasks (or fetch a single task when task_id is provided)."""
        args: Dict[str, Any] = {"group_id": group_id}
        if task_id:
            args["task_id"] = task_id
        return await ctx.daemon({"op": "task_list", "args": args})

    @app.get("/api/v1/groups/{group_id}/project_md")
    async def project_md_get(group_id: str) -> Dict[str, Any]:
        """Get PROJECT.md content for the group's active scope root (repo root)."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        active_scope_key = str(group.doc.get("active_scope_key") or "")

        project_root: Optional[str] = None
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            sk = str(sc.get("scope_key") or "")
            if sk == active_scope_key:
                project_root = str(sc.get("url") or "")
                break
        if not project_root:
            if scopes and isinstance(scopes[0], dict):
                project_root = str(scopes[0].get("url") or "")
        if not project_root:
            return {"ok": True, "result": {"found": False, "path": None, "content": None, "error": "No scope attached to group. Use 'cccc attach <path>' first."}}

        root = Path(project_root).expanduser()
        if not root.exists() or not root.is_dir():
            return {"ok": True, "result": {"found": False, "path": str(root / "PROJECT.md"), "content": None, "error": f"Project root does not exist: {root}"}}

        project_md_path = root / "PROJECT.md"
        if not project_md_path.exists():
            project_md_path_lower = root / "project.md"
            if project_md_path_lower.exists():
                project_md_path = project_md_path_lower
            else:
                return {"ok": True, "result": {"found": False, "path": str(project_md_path), "content": None, "error": f"PROJECT.md not found at {project_md_path}"}}

        try:
            content = project_md_path.read_text(encoding="utf-8", errors="replace")
            return {"ok": True, "result": {"found": True, "path": str(project_md_path), "content": content}}
        except Exception as e:
            return {"ok": True, "result": {"found": False, "path": str(project_md_path), "content": None, "error": f"Failed to read PROJECT.md: {e}"}}

    @app.put("/api/v1/groups/{group_id}/project_md")
    async def project_md_put(group_id: str, req: ProjectMdUpdateRequest) -> Dict[str, Any]:
        """Create or update PROJECT.md in the group's active scope root (repo root)."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
        active_scope_key = str(group.doc.get("active_scope_key") or "")

        project_root: Optional[str] = None
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            sk = str(sc.get("scope_key") or "")
            if sk == active_scope_key:
                project_root = str(sc.get("url") or "")
                break
        if not project_root:
            if scopes and isinstance(scopes[0], dict):
                project_root = str(scopes[0].get("url") or "")
        if not project_root:
            return {"ok": False, "error": {"code": "NO_SCOPE", "message": "No scope attached to group. Use 'cccc attach <path>' first."}}

        root = Path(project_root).expanduser()
        if not root.exists() or not root.is_dir():
            return {"ok": False, "error": {"code": "INVALID_SCOPE", "message": f"Project root does not exist: {root}"}}

        # Write to existing file if present; otherwise create PROJECT.md.
        project_md_path = root / "PROJECT.md"
        if not project_md_path.exists():
            project_md_path_lower = root / "project.md"
            if project_md_path_lower.exists():
                project_md_path = project_md_path_lower

        try:
            atomic_write_text(project_md_path, str(req.content or ""), encoding="utf-8")
            content = project_md_path.read_text(encoding="utf-8", errors="replace")
            return {"ok": True, "result": {"found": True, "path": str(project_md_path), "content": content}}
        except Exception as e:
            return {"ok": False, "error": {"code": "WRITE_FAILED", "message": f"Failed to write PROJECT.md: {e}"}}

    def _prompt_kind_to_filename(kind: str) -> str:
        k = str(kind or "").strip().lower()
        if k == "preamble":
            return PREAMBLE_FILENAME
        if k == "help":
            return HELP_FILENAME
        raise HTTPException(status_code=400, detail={"code": "invalid_kind", "message": f"unknown prompt kind: {kind}"})

    def _builtin_prompt_markdown(kind: str) -> str:
        k = str(kind or "").strip().lower()
        if k == "preamble":
            return str(DEFAULT_PREAMBLE_BODY or "").strip()
        if k == "help":
            return str(load_builtin_help_markdown() or "").strip()
        return ""

    @app.get("/api/v1/groups/{group_id}/prompts")
    async def prompts_get(group_id: str) -> Dict[str, Any]:
        """Get effective group guidance markdown (preamble/help) and override status."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        def _one(kind: str) -> Dict[str, Any]:
            filename = _prompt_kind_to_filename(kind)
            pf = read_group_prompt_file(group, filename)
            if pf.found and isinstance(pf.content, str) and pf.content.strip():
                return {"kind": kind, "source": "home", "filename": filename, "path": pf.path, "content": str(pf.content)}
            return {
                "kind": kind,
                "source": "builtin",
                "filename": filename,
                "path": pf.path,
                "content": _builtin_prompt_markdown(kind),
            }

        return {
            "ok": True,
            "result": {
                "preamble": _one("preamble"),
                "help": _one("help"),
            },
        }

    @app.put("/api/v1/groups/{group_id}/prompts/{kind}")
    async def prompts_put(group_id: str, kind: str, req: RepoPromptUpdateRequest) -> Dict[str, Any]:
        """Create or update a group prompt override file under CCCC_HOME."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        filename = _prompt_kind_to_filename(kind)
        try:
            raw = str(req.content or "")
            if not raw.strip():
                pf = delete_group_prompt_file(group, filename)
                return {"ok": True, "result": {"kind": kind, "source": "builtin", "filename": filename, "path": pf.path, "content": _builtin_prompt_markdown(kind)}}
            pf = write_group_prompt_file(group, filename, raw)
            return {"ok": True, "result": {"kind": kind, "source": "home", "filename": filename, "path": pf.path, "content": pf.content or ""}}
        except Exception as e:
            return {"ok": False, "error": {"code": "WRITE_FAILED", "message": f"Failed to write {filename}: {e}"}}

    @app.delete("/api/v1/groups/{group_id}/prompts/{kind}")
    async def prompts_delete(group_id: str, kind: str, confirm: str = "") -> Dict[str, Any]:
        """Reset a group prompt override by deleting the CCCC_HOME file (requires confirm=kind)."""
        if str(confirm or "").strip().lower() != str(kind or "").strip().lower():
            raise HTTPException(status_code=400, detail={"code": "confirmation_required", "message": f"confirm must equal kind: {kind}"})

        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        filename = _prompt_kind_to_filename(kind)
        try:
            pf = delete_group_prompt_file(group, filename)
            return {"ok": True, "result": {"kind": kind, "source": "builtin", "filename": filename, "path": pf.path, "content": _builtin_prompt_markdown(kind)}}
        except Exception as e:
            return {"ok": False, "error": {"code": "DELETE_FAILED", "message": f"Failed to delete {filename}: {e}"}}

    @app.post("/api/v1/groups/{group_id}/context")
    async def group_context_sync(group_id: str, request: Request) -> Dict[str, Any]:
        """Update group context via batch operations (v2).

        Body: {"ops": [{"op": "vision.update", "vision": "..."}, ...], "by": "user"}

        Supported ops:
        - vision.update, overview.manual.update
        - task.create/update/status/move/restore
        - agent.update/clear
        """
        try:
            body = await request.json()
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_json", "message": "invalid JSON body"})

        ops = body.get("ops") if isinstance(body.get("ops"), list) else []
        by = str(body.get("by") or "user")
        dry_run = coerce_bool(body.get("dry_run"), default=False)

        return await ctx.daemon({
            "op": "context_sync",
            "args": {"group_id": group_id, "ops": ops, "by": by, "dry_run": dry_run}
        })

    @app.get("/api/v1/groups/{group_id}/settings")
    async def group_settings_get(group_id: str) -> Dict[str, Any]:
        """Get group-scoped automation + delivery settings."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
        delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
        from ....kernel.terminal_transcript import get_terminal_transcript_settings
        from ....kernel.messaging import get_default_send_to

        tt = get_terminal_transcript_settings(group.doc)
        return {
            "ok": True,
            "result": {
                "settings": {
                    "default_send_to": get_default_send_to(group.doc),
                    "nudge_after_seconds": _safe_int(automation.get("nudge_after_seconds", 300), default=300, min_value=0),
                    "reply_required_nudge_after_seconds": _safe_int(automation.get("reply_required_nudge_after_seconds", 300), default=300, min_value=0),
                    "attention_ack_nudge_after_seconds": _safe_int(automation.get("attention_ack_nudge_after_seconds", 600), default=600, min_value=0),
                    "unread_nudge_after_seconds": _safe_int(automation.get("unread_nudge_after_seconds", 900), default=900, min_value=0),
                    "nudge_digest_min_interval_seconds": _safe_int(automation.get("nudge_digest_min_interval_seconds", 120), default=120, min_value=0),
                    "nudge_max_repeats_per_obligation": _safe_int(automation.get("nudge_max_repeats_per_obligation", 3), default=3, min_value=0),
                    "nudge_escalate_after_repeats": _safe_int(automation.get("nudge_escalate_after_repeats", 2), default=2, min_value=0),
                    "actor_idle_timeout_seconds": _safe_int(automation.get("actor_idle_timeout_seconds", 600), default=600, min_value=0),
                    "keepalive_delay_seconds": _safe_int(automation.get("keepalive_delay_seconds", 120), default=120, min_value=0),
                    "keepalive_max_per_actor": _safe_int(automation.get("keepalive_max_per_actor", 3), default=3, min_value=0),
                    "silence_timeout_seconds": _safe_int(automation.get("silence_timeout_seconds", 600), default=600, min_value=0),
                    "help_nudge_interval_seconds": _safe_int(automation.get("help_nudge_interval_seconds", 600), default=600, min_value=0),
                    "help_nudge_min_messages": _safe_int(automation.get("help_nudge_min_messages", 10), default=10, min_value=0),
                    "min_interval_seconds": _safe_int(delivery.get("min_interval_seconds", 0), default=0, min_value=0),
                    "auto_mark_on_delivery": coerce_bool(delivery.get("auto_mark_on_delivery"), default=False),
                    "terminal_transcript_visibility": str(tt.get("visibility") or "foreman"),
                    "terminal_transcript_notify_tail": coerce_bool(tt.get("notify_tail"), default=False),
                    "terminal_transcript_notify_lines": _safe_int(tt.get("notify_lines", 20), default=20, min_value=1, max_value=80),
                }
            }
        }

    @app.put("/api/v1/groups/{group_id}/settings")
    async def group_settings_update(group_id: str, req: GroupSettingsRequest) -> Dict[str, Any]:
        """Update group-scoped automation + delivery settings."""
        patch: Dict[str, Any] = {}
        if req.default_send_to is not None:
            patch["default_send_to"] = str(req.default_send_to)
        if req.nudge_after_seconds is not None:
            patch["nudge_after_seconds"] = max(0, req.nudge_after_seconds)
        if req.reply_required_nudge_after_seconds is not None:
            patch["reply_required_nudge_after_seconds"] = max(0, req.reply_required_nudge_after_seconds)
        if req.attention_ack_nudge_after_seconds is not None:
            patch["attention_ack_nudge_after_seconds"] = max(0, req.attention_ack_nudge_after_seconds)
        if req.unread_nudge_after_seconds is not None:
            patch["unread_nudge_after_seconds"] = max(0, req.unread_nudge_after_seconds)
        if req.nudge_digest_min_interval_seconds is not None:
            patch["nudge_digest_min_interval_seconds"] = max(0, req.nudge_digest_min_interval_seconds)
        if req.nudge_max_repeats_per_obligation is not None:
            patch["nudge_max_repeats_per_obligation"] = max(0, req.nudge_max_repeats_per_obligation)
        if req.nudge_escalate_after_repeats is not None:
            patch["nudge_escalate_after_repeats"] = max(0, req.nudge_escalate_after_repeats)
        if req.actor_idle_timeout_seconds is not None:
            patch["actor_idle_timeout_seconds"] = max(0, req.actor_idle_timeout_seconds)
        if req.keepalive_delay_seconds is not None:
            patch["keepalive_delay_seconds"] = max(0, req.keepalive_delay_seconds)
        if req.keepalive_max_per_actor is not None:
            patch["keepalive_max_per_actor"] = max(0, req.keepalive_max_per_actor)
        if req.silence_timeout_seconds is not None:
            patch["silence_timeout_seconds"] = max(0, req.silence_timeout_seconds)
        if req.help_nudge_interval_seconds is not None:
            patch["help_nudge_interval_seconds"] = max(0, req.help_nudge_interval_seconds)
        if req.help_nudge_min_messages is not None:
            patch["help_nudge_min_messages"] = max(0, req.help_nudge_min_messages)
        if req.min_interval_seconds is not None:
            patch["min_interval_seconds"] = max(0, req.min_interval_seconds)
        if req.auto_mark_on_delivery is not None:
            patch["auto_mark_on_delivery"] = bool(req.auto_mark_on_delivery)

        # Terminal transcript policy (group-scoped)
        if req.terminal_transcript_visibility is not None:
            patch["terminal_transcript_visibility"] = str(req.terminal_transcript_visibility)
        if req.terminal_transcript_notify_tail is not None:
            patch["terminal_transcript_notify_tail"] = bool(req.terminal_transcript_notify_tail)
        if req.terminal_transcript_notify_lines is not None:
            patch["terminal_transcript_notify_lines"] = max(1, min(80, int(req.terminal_transcript_notify_lines)))

        if not patch:
            return {"ok": True, "result": {"message": "no changes"}}

        return await ctx.daemon({
            "op": "group_settings_update",
            "args": {"group_id": group_id, "patch": patch, "by": req.by}
        })

    @app.get("/api/v1/groups/{group_id}/automation")
    async def group_automation_get(group_id: str) -> Dict[str, Any]:
        """Get group automation rules + snippets + runtime status."""
        return await ctx.daemon({"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}})

    @app.put("/api/v1/groups/{group_id}/automation")
    async def group_automation_update(group_id: str, req: GroupAutomationRequest) -> Dict[str, Any]:
        """Update group automation rules + snippets."""
        ruleset = AutomationRuleSet(rules=req.rules, snippets=req.snippets).model_dump()
        return await ctx.daemon(
            {
                "op": "group_automation_update",
                "args": {"group_id": group_id, "ruleset": ruleset, "expected_version": req.expected_version, "by": req.by},
            }
        )

    @app.post("/api/v1/groups/{group_id}/automation/manage")
    async def group_automation_manage(group_id: str, req: GroupAutomationManageRequest) -> Dict[str, Any]:
        """Manage group automation incrementally via actions."""
        return await ctx.daemon(
            {
                "op": "group_automation_manage",
                "args": {
                    "group_id": group_id,
                    "actions": [a for a in req.actions if isinstance(a, dict)],
                    "expected_version": req.expected_version,
                    "by": req.by,
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/automation/reset_baseline")
    async def group_automation_reset_baseline(group_id: str, req: GroupAutomationResetBaselineRequest) -> Dict[str, Any]:
        """Reset group automation rules/snippets to baseline defaults."""
        return await ctx.daemon(
            {
                "op": "group_automation_reset_baseline",
                "args": {
                    "group_id": group_id,
                    "expected_version": req.expected_version,
                    "by": req.by,
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/attach")
    async def group_attach(group_id: str, req: AttachRequest) -> Dict[str, Any]:
        return await ctx.daemon({"op": "attach", "args": {"path": req.path, "by": req.by, "group_id": group_id}})

    @app.delete("/api/v1/groups/{group_id}/scopes/{scope_key}")
    async def group_detach_scope(group_id: str, scope_key: str, by: str = "user") -> Dict[str, Any]:
        """Detach a scope from a group."""
        return await ctx.daemon({"op": "group_detach_scope", "args": {"group_id": group_id, "scope_key": scope_key, "by": by}})

    @app.get("/api/v1/groups/{group_id}/ledger/tail")
    async def ledger_tail(
        group_id: str,
        lines: int = 50,
        with_read_status: bool = False,
        with_ack_status: bool = False,
        with_obligation_status: bool = False,
    ) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        raw_lines = read_last_lines(group.ledger_path, int(lines))
        events = []
        for ln in raw_lines:
            try:
                events.append(json.loads(ln))
            except Exception:
                continue

        # Optionally include read status for chat.message events (batch optimized)
        if with_read_status:
            from ....kernel.inbox import get_read_status_batch
            status_map = get_read_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in status_map:
                    ev["_read_status"] = status_map[event_id]

        # Optionally include ack status for attention chat.message events (batch optimized)
        if with_ack_status:
            from ....kernel.inbox import get_ack_status_batch
            ack_map = get_ack_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in ack_map:
                    ev["_ack_status"] = ack_map[event_id]

        if with_obligation_status:
            from ....kernel.inbox import get_obligation_status_batch
            obligation_map = get_obligation_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in obligation_map:
                    ev["_obligation_status"] = obligation_map[event_id]

        return {"ok": True, "result": {"events": events}}

    @app.get("/api/v1/groups/{group_id}/ledger/search")
    async def ledger_search(
        group_id: str,
        q: str = "",
        kind: str = "all",
        by: str = "",
        before: str = "",
        after: str = "",
        limit: int = 50,
        with_read_status: bool = False,
        with_ack_status: bool = False,
        with_obligation_status: bool = False,
    ) -> Dict[str, Any]:
        """Search and paginate messages in the ledger.

        Query params:
        - q: Text search query (case-insensitive substring match)
        - kind: Filter by message type (all/chat/notify)
        - by: Filter by sender (actor_id or "user")
        - before: Return messages before this event_id (backward pagination)
        - after: Return messages after this event_id (forward pagination)
        - limit: Maximum number of messages to return (default 50, max 200)
        - with_read_status: Include read status for each message
        """
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        from ....kernel.inbox import search_messages, get_read_status_batch

        # Validate and clamp limit
        limit = max(1, min(200, limit))

        # Validate kind filter
        kind_filter = kind if kind in ("all", "chat", "notify") else "all"

        events, has_more = search_messages(
            group,
            query=q,
            kind_filter=kind_filter,  # type: ignore
            by_filter=by,
            before_id=before,
            after_id=after,
            limit=limit,
        )

        # Optionally include read status (batch optimized)
        if with_read_status:
            status_map = get_read_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in status_map:
                    ev["_read_status"] = status_map[event_id]

        # Optionally include ack status (batch optimized)
        if with_ack_status:
            from ....kernel.inbox import get_ack_status_batch
            ack_map = get_ack_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in ack_map:
                    ev["_ack_status"] = ack_map[event_id]

        if with_obligation_status:
            from ....kernel.inbox import get_obligation_status_batch
            obligation_map = get_obligation_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in obligation_map:
                    ev["_obligation_status"] = obligation_map[event_id]

        return {
            "ok": True,
            "result": {
                "events": events,
                "has_more": has_more,
                "count": len(events),
            }
        }

    @app.get("/api/v1/groups/{group_id}/ledger/window")
    async def ledger_window(
        group_id: str,
        center: str,
        kind: str = "chat",
        before: int = 30,
        after: int = 30,
        with_read_status: bool = False,
        with_ack_status: bool = False,
        with_obligation_status: bool = False,
    ) -> Dict[str, Any]:
        """Return a bounded window of events around a center event_id.

        This is used for "jump-to message" deep links and search result navigation.
        """
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        from ....kernel.inbox import find_event, search_messages, get_read_status_batch

        center_id = str(center or "").strip()
        if not center_id:
            raise HTTPException(status_code=400, detail={"code": "missing_center", "message": "missing center event_id"})

        center_event = find_event(group, center_id)
        if center_event is None:
            raise HTTPException(status_code=404, detail={"code": "event_not_found", "message": f"event not found: {center_id}"})

        # Validate and clamp window sizes
        before = max(0, min(200, int(before)))
        after = max(0, min(200, int(after)))

        kind_filter = kind if kind in ("all", "chat", "notify") else "chat"

        if kind_filter == "chat" and str(center_event.get("kind") or "") != "chat.message":
            raise HTTPException(status_code=400, detail={"code": "invalid_center_kind", "message": "center event kind must be chat.message for kind=chat"})

        before_events, has_more_before = search_messages(
            group,
            query="",
            kind_filter=kind_filter,  # type: ignore
            before_id=center_id,
            limit=before,
        )
        after_events, has_more_after = search_messages(
            group,
            query="",
            kind_filter=kind_filter,  # type: ignore
            after_id=center_id,
            limit=after,
        )

        events = [*before_events, center_event, *after_events]

        if with_read_status:
            status_map = get_read_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in status_map:
                    ev["_read_status"] = status_map[event_id]

        if with_ack_status:
            from ....kernel.inbox import get_ack_status_batch
            ack_map = get_ack_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in ack_map:
                    ev["_ack_status"] = ack_map[event_id]

        if with_obligation_status:
            from ....kernel.inbox import get_obligation_status_batch
            obligation_map = get_obligation_status_batch(group, events)
            for ev in events:
                event_id = str(ev.get("id") or "")
                if event_id in obligation_map:
                    ev["_obligation_status"] = obligation_map[event_id]

        return {
            "ok": True,
            "result": {
                "center_id": center_id,
                "center_index": len(before_events),
                "events": events,
                "has_more_before": has_more_before,
                "has_more_after": has_more_after,
                "count": len(events),
            },
        }

    @app.get("/api/v1/groups/{group_id}/events/{event_id}/read_status")
    async def event_read_status(group_id: str, event_id: str) -> Dict[str, Any]:
        """Get read status for a specific event (which actors have read it)."""
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        from ....kernel.inbox import get_read_status
        status = get_read_status(group, event_id)
        return {"ok": True, "result": {"event_id": event_id, "read_status": status}}

    @app.get("/api/v1/groups/{group_id}/ledger/stream")
    async def ledger_stream(group_id: str) -> StreamingResponse:
        from ..streams import sse_ledger_tail, create_sse_response
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        return create_sse_response(sse_ledger_tail(group.ledger_path))

    @app.get("/api/v1/events/stream")
    async def global_events_stream() -> StreamingResponse:
        """SSE stream for global events (group created/deleted, etc.)."""
        from ..streams import sse_global_events_tail, create_sse_response
        return create_sse_response(sse_global_events_tail(ctx.home))
