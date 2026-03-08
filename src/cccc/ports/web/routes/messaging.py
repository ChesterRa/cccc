from __future__ import annotations

import json
from typing import Any, Dict

from fastapi import APIRouter, Depends, File, Form, HTTPException, Request, UploadFile
from fastapi.responses import FileResponse

from ....kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ....kernel.group import load_group
from ..schemas import (
    ReplyRequest,
    RouteContext,
    SendCrossGroupRequest,
    SendRequest,
    UserAckRequest,
    WEB_MAX_FILE_BYTES,
    WEB_MAX_FILE_MB,
    _normalize_reply_required,
    check_group,
    require_group,
)


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])

    @group_router.post("/send")
    async def send(group_id: str, req: SendRequest) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "send",
                "args": {
                    "group_id": group_id,
                    "text": req.text,
                    "by": req.by,
                    "to": list(req.to),
                    "path": req.path,
                    "priority": req.priority,
                    "reply_required": _normalize_reply_required(req.reply_required),
                    "src_group_id": req.src_group_id,
                    "src_event_id": req.src_event_id,
                },
            }
        )

    @group_router.post("/send_cross_group")
    async def send_cross_group(request: Request, group_id: str, req: SendCrossGroupRequest) -> Dict[str, Any]:
        """Send a message to another group with provenance.

        This creates a source chat.message in the current group and forwards a copy into the destination group
        with (src_group_id, src_event_id) set.
        """
        check_group(request, req.dst_group_id)
        return await ctx.daemon(
            {
                "op": "send_cross_group",
                "args": {
                    "group_id": group_id,
                    "dst_group_id": req.dst_group_id,
                    "text": req.text,
                    "by": req.by,
                    "to": list(req.to),
                    "priority": req.priority,
                    "reply_required": _normalize_reply_required(req.reply_required),
                },
            }
        )

    @group_router.post("/reply")
    async def reply(group_id: str, req: ReplyRequest) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "reply",
                "args": {
                    "group_id": group_id,
                    "text": req.text,
                    "by": req.by,
                    "to": list(req.to),
                    "reply_to": req.reply_to,
                    "priority": req.priority,
                    "reply_required": _normalize_reply_required(req.reply_required),
                },
            }
        )

    @group_router.post("/events/{event_id}/ack")
    async def chat_ack(group_id: str, event_id: str, req: UserAckRequest) -> Dict[str, Any]:
        # Web UI can only ACK as user (no impersonation).
        if str(req.by or "").strip() != "user":
            raise HTTPException(status_code=403, detail={"code": "permission_denied", "message": "ack is only supported as user in the web UI"})
        return await ctx.daemon(
            {
                "op": "chat_ack",
                "args": {"group_id": group_id, "event_id": event_id, "actor_id": "user", "by": "user"},
            }
        )

    @group_router.post("/send_upload")
    async def send_upload(
        group_id: str,
        by: str = Form("user"),
        text: str = Form(""),
        to_json: str = Form("[]"),
        path: str = Form(""),
        priority: str = Form("normal"),
        reply_required: str = Form("false"),
        files: list[UploadFile] = File(default_factory=list),
    ) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        try:
            parsed_to = json.loads(to_json or "[]")
        except Exception:
            parsed_to = []
        to_list = [str(x).strip() for x in (parsed_to if isinstance(parsed_to, list) else []) if str(x).strip()]

        # Preflight recipients before storing attachments (avoid orphan blobs on invalid/no-op sends).
        from ....kernel.actors import resolve_recipient_tokens, list_actors
        from ....kernel.messaging import get_default_send_to
        try:
            canonical_to = resolve_recipient_tokens(group, to_list)
        except Exception as e:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": str(e)})
        if to_list and not canonical_to:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": "invalid recipient"})

        raw_text = str(text or "").strip()
        if not canonical_to and not to_list and raw_text:
            import re
            mention_pattern = re.compile(r"@(\w[\w-]*)")
            mentions = mention_pattern.findall(raw_text)
            if mentions:
                actor_ids = {str(a.get("id") or "").strip() for a in list_actors(group) if isinstance(a, dict)}
                mention_tokens: list[str] = []
                for m in mentions:
                    if not m:
                        continue
                    if m in ("all", "peers", "foreman"):
                        mention_tokens.append(f"@{m}")
                    elif m in actor_ids:
                        mention_tokens.append(m)
                if mention_tokens:
                    try:
                        canonical_to = resolve_recipient_tokens(group, mention_tokens)
                    except Exception:
                        canonical_to = []

        if not canonical_to and not to_list and get_default_send_to(group.doc) == "foreman":
            canonical_to = ["@foreman"]

        # Note: enabled-recipient validation + auto-wake is handled by the daemon.

        attachments: list[dict[str, Any]] = []
        for f in files or []:
            raw = await f.read()
            if len(raw) > WEB_MAX_FILE_BYTES:
                raise HTTPException(
                    status_code=413,
                    detail={"code": "file_too_large", "message": f"file too large (> {WEB_MAX_FILE_MB}MB)"},
                )
            attachments.append(
                store_blob_bytes(
                    group,
                    data=raw,
                    filename=str(getattr(f, "filename", "") or "file"),
                    mime_type=str(getattr(f, "content_type", "") or ""),
                )
            )

        msg_text = str(text or "").strip()
        if not msg_text and attachments:
            if len(attachments) == 1:
                msg_text = f"[file] {attachments[0].get('title') or 'file'}"
            else:
                msg_text = f"[files] {len(attachments)} attachments"

        prio = str(priority or "normal").strip() or "normal"
        if prio not in ("normal", "attention"):
            raise HTTPException(status_code=400, detail={"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"})

        return await ctx.daemon(
            {
                "op": "send",
                "args": {
                    "group_id": group_id,
                    "text": msg_text,
                    "by": by,
                    "to": canonical_to,
                    "path": path,
                    "attachments": attachments,
                    "priority": prio,
                    "reply_required": _normalize_reply_required(reply_required),
                },
            }
        )

    @group_router.post("/reply_upload")
    async def reply_upload(
        group_id: str,
        by: str = Form("user"),
        text: str = Form(""),
        to_json: str = Form("[]"),
        reply_to: str = Form(""),
        priority: str = Form("normal"),
        reply_required: str = Form("false"),
        files: list[UploadFile] = File(default_factory=list),
    ) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        reply_to_id = str(reply_to or "").strip()
        if not reply_to_id:
            raise HTTPException(status_code=400, detail={"code": "missing_reply_to", "message": "missing reply_to"})

        try:
            parsed_to = json.loads(to_json or "[]")
        except Exception:
            parsed_to = []
        to_list = [str(x).strip() for x in (parsed_to if isinstance(parsed_to, list) else []) if str(x).strip()]

        # Preflight recipients before storing attachments (avoid orphan blobs on invalid/no-op sends).
        from ....kernel.actors import resolve_recipient_tokens
        from ....kernel.inbox import find_event
        from ....kernel.messaging import default_reply_recipients

        original = find_event(group, reply_to_id)
        if original is None:
            raise HTTPException(status_code=404, detail={"code": "event_not_found", "message": f"event not found: {reply_to_id}"})

        try:
            canonical_to = resolve_recipient_tokens(group, to_list)
        except Exception as e:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": str(e)})
        if to_list and not canonical_to:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": "invalid recipient"})

        if not canonical_to and not to_list:
            canonical_to = resolve_recipient_tokens(group, default_reply_recipients(group, by=by, original_event=original))

        # Note: enabled-recipient validation + auto-wake is handled by the daemon.

        attachments: list[dict[str, Any]] = []
        for f in files or []:
            raw = await f.read()
            if len(raw) > WEB_MAX_FILE_BYTES:
                raise HTTPException(
                    status_code=413,
                    detail={"code": "file_too_large", "message": f"file too large (> {WEB_MAX_FILE_MB}MB)"},
                )
            attachments.append(
                store_blob_bytes(
                    group,
                    data=raw,
                    filename=str(getattr(f, "filename", "") or "file"),
                    mime_type=str(getattr(f, "content_type", "") or ""),
                )
            )

        msg_text = str(text or "").strip()
        if not msg_text and attachments:
            if len(attachments) == 1:
                msg_text = f"[file] {attachments[0].get('title') or 'file'}"
            else:
                msg_text = f"[files] {len(attachments)} attachments"

        prio = str(priority or "normal").strip() or "normal"
        if prio not in ("normal", "attention"):
            raise HTTPException(status_code=400, detail={"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"})

        return await ctx.daemon(
            {
                "op": "reply",
                "args": {
                    "group_id": group_id,
                    "text": msg_text,
                    "by": by,
                    "to": canonical_to,
                    "reply_to": reply_to_id,
                    "attachments": attachments,
                    "priority": prio,
                    "reply_required": _normalize_reply_required(reply_required),
                },
            }
        )

    @group_router.get("/blobs/{blob_name}")
    async def blob_download(group_id: str, blob_name: str) -> FileResponse:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        name = str(blob_name or "").strip()
        if not name or "/" in name or "\\" in name or ".." in name:
            raise HTTPException(status_code=400, detail={"code": "invalid_blob", "message": "invalid blob name"})

        rel = f"state/blobs/{name}"
        try:
            abs_path = resolve_blob_attachment_path(group, rel_path=rel)
        except Exception:
            raise HTTPException(status_code=400, detail={"code": "invalid_blob", "message": "invalid blob name"})

        if not abs_path.exists() or not abs_path.is_file():
            raise HTTPException(status_code=404, detail={"code": "not_found", "message": "blob not found"})

        download_name = name
        if len(name) > 64 and "_" in name:
            # blob name format: <sha256>_<filename>
            download_name = name.split("_", 1)[1] or name
        return FileResponse(path=abs_path, filename=download_name)

    return [group_router]
