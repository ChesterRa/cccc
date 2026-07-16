from __future__ import annotations

import json
from typing import Any, Dict

from fastapi import APIRouter, Depends, File, Form, HTTPException, Request, UploadFile
from fastapi.responses import FileResponse

from ....daemon.group_bridge.route_lookup import resolve_remote_group_route
from ....kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ....kernel.group import load_group
from ..schemas import (
    DelegateContactRequest,
    ReplyRequest,
    RouteContext,
    SendCrossGroupRequest,
    SendRequest,
    TrackedSendRequest,
    UserAckRequest,
    WEB_MAX_FILE_BYTES,
    WEB_MAX_FILE_MB,
    _normalize_reply_required,
    check_group,
    require_group,
)

def create_routers(ctx: RouteContext) -> list[APIRouter]:
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])

    def _parse_refs_json(raw: str) -> list[dict[str, Any]]:
        text = str(raw or "").strip()
        if not text:
            return []
        try:
            parsed = json.loads(text)
        except Exception as exc:
            raise HTTPException(status_code=400, detail={"code": "invalid_refs", "message": str(exc)})
        if not isinstance(parsed, list):
            raise HTTPException(status_code=400, detail={"code": "invalid_refs", "message": "refs_json must be a JSON array"})
        refs: list[dict[str, Any]] = []
        for item in parsed:
            if isinstance(item, dict):
                refs.append(item)
        return refs

    def _normalize_priority(raw: str) -> str:
        prio = str(raw or "normal").strip() or "normal"
        if prio not in ("normal", "attention"):
            raise HTTPException(status_code=400, detail={"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"})
        return prio

    def _normalize_client_id(raw: str) -> str:
        return str(raw or "").strip()

    def _build_message_request(op: str, *, group_id: str, args: Dict[str, Any]) -> Dict[str, Any]:
        return {"op": op, "args": {"group_id": group_id, **args}}

    async def _submit_message(req: Dict[str, Any]) -> Dict[str, Any]:
        return await ctx.daemon(req)

    async def _store_upload_attachments(group: Any, files: list[UploadFile]) -> list[dict[str, Any]]:
        attachments: list[dict[str, Any]] = []
        for upload in files or []:
            raw = await upload.read()
            if len(raw) > WEB_MAX_FILE_BYTES:
                raise HTTPException(
                    status_code=413,
                    detail={"code": "file_too_large", "message": f"file too large (> {WEB_MAX_FILE_MB}MB)"},
                )
            attachments.append(
                store_blob_bytes(
                    group,
                    data=raw,
                    filename=str(getattr(upload, "filename", "") or "file"),
                    mime_type=str(getattr(upload, "content_type", "") or ""),
                )
            )
        return attachments

    def _message_text_for_upload(*, text: str, attachments: list[dict[str, Any]]) -> str:
        msg_text = str(text or "").strip()
        if msg_text or not attachments:
            return msg_text
        if len(attachments) == 1:
            return f"[file] {attachments[0].get('title') or 'file'}"
        return f"[files] {len(attachments)} attachments"

    @group_router.post("/send")
    async def send(group_id: str, req: SendRequest) -> Dict[str, Any]:
        daemon_req = _build_message_request(
            "send",
            group_id=group_id,
            args={
                "text": req.text,
                "by": req.by,
                "to": list(req.to),
                "path": req.path,
                "quote_text": req.quote_text,
                "priority": req.priority,
                "reply_required": _normalize_reply_required(req.reply_required),
                "source_platform": req.source_platform,
                "source_user_name": req.source_user_name,
                "source_user_id": req.source_user_id,
                "src_group_id": req.src_group_id,
                "src_event_id": req.src_event_id,
                "source_multiaddrs": list(req.source_multiaddrs),
                "client_id": _normalize_client_id(req.client_id),
                "refs": list(req.refs),
                "suggested_user_message": req.suggested_user_message,
            },
        )
        return await _submit_message(daemon_req)

    @group_router.post("/send_cross_group")
    async def send_cross_group(request: Request, group_id: str, req: SendCrossGroupRequest) -> Dict[str, Any]:
        """Send a message to another group with provenance.

        This creates a source chat.message in the current group and forwards a copy into the destination group
        with (src_group_id, src_event_id) set.
        """
        if resolve_remote_group_route(group_id=group_id, remote_group_id=req.dst_group_id) is None:
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
                    "reply_to": req.reply_to,
                    "quote_text": req.quote_text,
                    "client_id": _normalize_client_id(req.client_id),
                    "remote_reply_to_event_id": req.remote_reply_to_event_id,
                    "attachments": list(req.attachments),
                },
            }
        )

    @group_router.post("/send_cross_group_upload")
    async def send_cross_group_upload(
        request: Request,
        group_id: str,
        dst_group_id: str = Form(""),
        by: str = Form("user"),
        text: str = Form(""),
        to_json: str = Form("[]"),
        priority: str = Form("normal"),
        reply_required: str = Form("false"),
        reply_to: str = Form(""),
        quote_text: str = Form(""),
        client_id: str = Form(""),
        remote_reply_to_event_id: str = Form(""),
        files: list[UploadFile] = File(default_factory=list),
    ) -> Dict[str, Any]:
        """Send uploaded attachments to a trusted remote Group Bridge target."""
        dst_gid = str(dst_group_id or "").strip()
        if not dst_gid:
            raise HTTPException(status_code=400, detail={"code": "missing_dst_group_id", "message": "missing dst_group_id"})
        if resolve_remote_group_route(group_id=group_id, remote_group_id=dst_gid) is None:
            check_group(request, dst_gid)
            raise HTTPException(
                status_code=400,
                detail={
                    "code": "attachments_not_supported",
                    "message": "attachments are only supported for remote Group Bridge messages",
                },
            )
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})
        try:
            parsed_to = json.loads(to_json or "[]")
        except Exception:
            parsed_to = []
        to_list = [str(x).strip() for x in (parsed_to if isinstance(parsed_to, list) else []) if str(x).strip()]
        attachments = await _store_upload_attachments(group, files)
        msg_text = _message_text_for_upload(text=text, attachments=attachments)
        return await ctx.daemon(
            {
                "op": "send_cross_group",
                "args": {
                    "group_id": group_id,
                    "dst_group_id": dst_gid,
                    "text": msg_text,
                    "by": by,
                    "to": to_list,
                    "priority": _normalize_priority(priority),
                    "reply_required": _normalize_reply_required(reply_required),
                    "reply_to": str(reply_to or "").strip(),
                    "quote_text": str(quote_text or "").strip(),
                    "client_id": _normalize_client_id(client_id),
                    "remote_reply_to_event_id": str(remote_reply_to_event_id or "").strip(),
                    "attachments": attachments,
                },
            }
        )

    @group_router.post("/delegate_contact")
    async def delegate_contact(request: Request, group_id: str, req: DelegateContactRequest) -> Dict[str, Any]:
        """Ask a local-group agent to contact a target group on the user's behalf.

        The user's own message stays in the local group; this triggers a
        deterministic relay authored by a local agent into the target group.
        """
        check_group(request, req.dst_group_id)
        return await ctx.daemon(
            {
                "op": "relay_user_delegation",
                "args": {
                    "group_id": group_id,
                    "dst_group_id": req.dst_group_id,
                    "text": req.text,
                    "by": req.by,
                    "delegation_token": req.delegation_token,
                    "relay_sender": req.relay_sender,
                    "source_event_id": req.source_event_id,
                    "target_actor": req.target_actor,
                    "contact_text": req.contact_text,
                },
            }
        )

    @group_router.post("/tracked_send")
    async def tracked_send(group_id: str, req: TrackedSendRequest) -> Dict[str, Any]:
        daemon_req = _build_message_request(
            "tracked_send",
            group_id=group_id,
            args={
                "title": req.title,
                "text": req.text,
                "by": req.by,
                "to": list(req.to),
                "outcome": req.outcome,
                "checklist": list(req.checklist),
                "assignee": req.assignee,
                "waiting_on": req.waiting_on,
                "handoff_to": req.handoff_to,
                "notes": req.notes,
                "priority": req.priority,
                "reply_required": _normalize_reply_required(req.reply_required),
                "idempotency_key": _normalize_client_id(req.idempotency_key),
                "refs": list(req.refs),
            },
        )
        return await _submit_message(daemon_req)

    @group_router.post("/reply")
    async def reply(group_id: str, req: ReplyRequest) -> Dict[str, Any]:
        daemon_req = _build_message_request(
            "reply",
            group_id=group_id,
            args={
                "text": req.text,
                "by": req.by,
                "to": list(req.to),
                "reply_to": req.reply_to,
                "priority": req.priority,
                "reply_required": _normalize_reply_required(req.reply_required),
                "client_id": _normalize_client_id(req.client_id),
                "refs": list(req.refs),
                "suggested_user_message": req.suggested_user_message,
            },
        )
        return await _submit_message(daemon_req)

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
        client_id: str = Form(""),
        refs_json: str = Form("[]"),
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
        from ....kernel.actors import resolve_recipient_tokens
        from ....kernel.messaging import get_default_send_to
        try:
            canonical_to = resolve_recipient_tokens(group, to_list)
        except Exception as e:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": str(e)})
        if to_list and not canonical_to:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": "invalid recipient"})

        if not canonical_to and not to_list and get_default_send_to(group.doc) == "foreman":
            canonical_to = ["@foreman"]

        # Note: enabled-recipient validation + auto-wake is handled by the daemon.

        attachments = await _store_upload_attachments(group, files)
        msg_text = _message_text_for_upload(text=text, attachments=attachments)
        prio = _normalize_priority(priority)
        refs = _parse_refs_json(refs_json)
        normalized_client_id = _normalize_client_id(client_id)
        daemon_req = _build_message_request(
            "send",
            group_id=group_id,
            args={
                "text": msg_text,
                "by": by,
                "to": canonical_to,
                "path": path,
                "attachments": attachments,
                "priority": prio,
                "reply_required": _normalize_reply_required(reply_required),
                "client_id": normalized_client_id,
                "refs": refs,
            },
        )
        return await _submit_message(daemon_req)

    @group_router.post("/reply_upload")
    async def reply_upload(
        group_id: str,
        by: str = Form("user"),
        text: str = Form(""),
        to_json: str = Form("[]"),
        reply_to: str = Form(""),
        priority: str = Form("normal"),
        reply_required: str = Form("false"),
        client_id: str = Form(""),
        refs_json: str = Form("[]"),
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
        from ....daemon.group_bridge.reply_relay import can_relay_group_bridge_reply, default_group_bridge_reply_recipients

        original = find_event(group, reply_to_id)
        if original is None:
            raise HTTPException(status_code=404, detail={"code": "event_not_found", "message": f"event not found: {reply_to_id}"})
        original_data = original.get("data") if isinstance(original.get("data"), dict) else {}

        try:
            canonical_to = resolve_recipient_tokens(group, to_list)
        except Exception as e:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": str(e)})
        if to_list and not canonical_to:
            raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": "invalid recipient"})

        if not canonical_to and not to_list:
            try:
                relayable_group_bridge_reply = can_relay_group_bridge_reply(group_id=group.group_id, original_data=original_data)
                if relayable_group_bridge_reply:
                    group_bridge_reply_to = default_group_bridge_reply_recipients(original_data)
                    if not group_bridge_reply_to:
                        raise HTTPException(
                            status_code=400,
                            detail={
                                "code": "missing_remote_recipient",
                                "message": "Group Bridge replies require an explicit recipient.",
                            },
                        )
                    canonical_to = resolve_recipient_tokens(group, ["user"])
                else:
                    canonical_to = resolve_recipient_tokens(group, default_reply_recipients(group, by=by, original_event=original))
            except Exception as e:
                if isinstance(e, HTTPException):
                    raise
                raise HTTPException(status_code=400, detail={"code": "invalid_recipient", "message": str(e)})

        # Note: enabled-recipient validation + auto-wake is handled by the daemon.

        attachments = await _store_upload_attachments(group, files)
        msg_text = _message_text_for_upload(text=text, attachments=attachments)
        prio = _normalize_priority(priority)
        refs = _parse_refs_json(refs_json)
        normalized_client_id = _normalize_client_id(client_id)
        daemon_req = _build_message_request(
            "reply",
            group_id=group_id,
            args={
                "text": msg_text,
                "by": by,
                "to": to_list,
                "reply_to": reply_to_id,
                "attachments": attachments,
                "priority": prio,
                "reply_required": _normalize_reply_required(reply_required),
                "client_id": normalized_client_id,
                "refs": refs,
            },
        )
        return await _submit_message(daemon_req)

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
        response = FileResponse(path=abs_path, filename=download_name)
        # Blob names are content-addressed (<sha256>_<filename>), so they are safe to cache aggressively.
        response.headers["Cache-Control"] = "private, max-age=31536000, immutable"
        return response

    return [group_router]
