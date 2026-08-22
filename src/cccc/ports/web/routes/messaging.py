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
    MessageDeliverRequest,
    ReplyRequest,
    ReplyRequestCancelRequest,
    RouteContext,
    SendCrossGroupRequest,
    SendRequest,
    TrackedSendRequest,
    WEB_MAX_FILE_BYTES,
    WEB_MAX_FILE_MB,
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

    def _normalize_message_mode(raw: str) -> str:
        mode = str(raw or "send").strip().replace("-", "_") or "send"
        if mode not in {"send", "request_reply", "mail"}:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_message_mode", "message": "message_mode must be send, request_reply, or mail"},
            )
        return mode

    def _normalize_reply_message_mode(raw: str) -> str:
        mode = str(raw or "send").strip().replace("-", "_") or "send"
        if mode not in {"send", "mail"}:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_message_mode", "message": "reply message_mode must be send or mail"},
            )
        return mode

    def _normalize_client_id(raw: str) -> str:
        return str(raw or "").strip()

    def _parse_recipients_json(raw: str) -> list[str]:
        try:
            parsed = json.loads(raw or "[]")
        except Exception as exc:
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_recipient", "message": f"to_json must be a JSON array: {exc}"},
            )
        if not isinstance(parsed, list):
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_recipient", "message": "to_json must be a JSON array"},
            )
        if any(not isinstance(item, str) for item in parsed):
            raise HTTPException(
                status_code=400,
                detail={"code": "invalid_recipient", "message": "to_json entries must be strings"},
            )
        return [str(item).strip() for item in parsed if isinstance(item, str) and str(item).strip()]

    def _build_message_request(op: str, *, group_id: str, args: Dict[str, Any]) -> Dict[str, Any]:
        return {"op": op, "args": {"group_id": group_id, **args}}

    async def _submit_message(req: Dict[str, Any]) -> Dict[str, Any]:
        return await ctx.daemon(req)

    async def _preflight_upload(*, operation: str, group_id: str, args: Dict[str, Any]) -> Dict[str, Any] | None:
        response = await ctx.daemon(
            _build_message_request(
                "message_upload_preflight",
                group_id=group_id,
                args={"operation": operation, **args},
            )
        )
        if not bool(response.get("ok")):
            error = response.get("error") if isinstance(response.get("error"), dict) else {}
            code = str(error.get("code") or "invalid_request")
            status_code = 400
            if "not_found" in code:
                status_code = 404
            elif "permission" in code:
                status_code = 403
            elif code.endswith(("_busy", "_conflict", "_lease_lost")):
                status_code = 409
            raise HTTPException(
                status_code=status_code,
                detail={
                    "code": code,
                    "message": str(error.get("message") or "message upload preflight failed"),
                    "details": error.get("details") if isinstance(error.get("details"), dict) else {},
                },
            )
        result = response.get("result") if isinstance(response.get("result"), dict) else {}
        if bool(result.get("duplicate")):
            existing = result.get("result") if isinstance(result.get("result"), dict) else {}
            return {"ok": True, "result": existing}
        return None

    async def _store_upload_attachments(group: Any, files: list[UploadFile]) -> list[dict[str, Any]]:
        staged: list[tuple[bytes, str, str]] = []
        for upload in files or []:
            raw = await upload.read()
            if len(raw) > WEB_MAX_FILE_BYTES:
                raise HTTPException(
                    status_code=413,
                    detail={"code": "file_too_large", "message": f"file too large (> {WEB_MAX_FILE_MB}MB)"},
                )
            staged.append(
                (
                    raw,
                    str(getattr(upload, "filename", "") or "file"),
                    str(getattr(upload, "content_type", "") or ""),
                )
            )
        return [
            store_blob_bytes(
                group,
                data=raw,
                filename=filename,
                mime_type=mime_type,
            )
            for raw, filename, mime_type in staged
        ]

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
                "message_mode": req.message_mode,
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
                    "message_mode": req.message_mode,
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
        message_mode: str = Form("send"),
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
                    "message_mode": _normalize_message_mode(message_mode),
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
                "task_priority": req.task_priority,
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
                "message_mode": req.message_mode,
                "client_id": _normalize_client_id(req.client_id),
                "refs": list(req.refs),
                "suggested_user_message": req.suggested_user_message,
            },
        )
        return await _submit_message(daemon_req)

    @group_router.post("/messages/{source_event_id}/deliver")
    async def message_deliver(
        group_id: str,
        source_event_id: str,
        req: MessageDeliverRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "message_deliver",
                "args": {
                    "group_id": group_id,
                    "source_event_id": source_event_id,
                    "actor_ids": list(req.actor_ids),
                    "force_ambiguous": req.force_ambiguous,
                    "by": "user",
                },
            }
        )

    @group_router.post("/messages/{source_event_id}/reply-request/cancel")
    async def reply_request_cancel(
        group_id: str,
        source_event_id: str,
        _req: ReplyRequestCancelRequest,
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "reply_request_cancel",
                "args": {
                    "group_id": group_id,
                    "source_event_id": source_event_id,
                    "by": "user",
                },
            }
        )

    @group_router.post("/send_upload")
    async def send_upload(
        group_id: str,
        by: str = Form("user"),
        text: str = Form(""),
        to_json: str = Form("[]"),
        path: str = Form(""),
        message_mode: str = Form("send"),
        client_id: str = Form(""),
        refs_json: str = Form("[]"),
        files: list[UploadFile] = File(default_factory=list),
    ) -> Dict[str, Any]:
        group = load_group(group_id)
        if group is None:
            raise HTTPException(status_code=404, detail={"code": "group_not_found", "message": f"group not found: {group_id}"})

        to_list = _parse_recipients_json(to_json)
        mode = _normalize_message_mode(message_mode)
        refs = _parse_refs_json(refs_json)
        normalized_client_id = _normalize_client_id(client_id)
        replay = await _preflight_upload(
            operation="send",
            group_id=group_id,
            args={
                "text": text,
                "by": by,
                "to": to_list,
                "path": path,
                "message_mode": mode,
                "client_id": normalized_client_id,
                "refs": refs,
                "has_attachments": bool(files),
            },
        )
        if replay is not None:
            return replay
        attachments = await _store_upload_attachments(group, files)
        msg_text = _message_text_for_upload(text=text, attachments=attachments)
        daemon_req = _build_message_request(
            "send",
            group_id=group_id,
            args={
                "text": msg_text,
                "by": by,
                "to": to_list,
                "path": path,
                "attachments": attachments,
                "message_mode": mode,
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
        message_mode: str = Form("send"),
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
        mode = _normalize_reply_message_mode(message_mode)

        to_list = _parse_recipients_json(to_json)
        refs = _parse_refs_json(refs_json)
        normalized_client_id = _normalize_client_id(client_id)
        replay = await _preflight_upload(
            operation="reply",
            group_id=group_id,
            args={
                "text": text,
                "by": by,
                "to": to_list,
                "reply_to": reply_to_id,
                "message_mode": mode,
                "client_id": normalized_client_id,
                "refs": refs,
                "has_attachments": bool(files),
            },
        )
        if replay is not None:
            return replay
        attachments = await _store_upload_attachments(group, files)
        msg_text = _message_text_for_upload(text=text, attachments=attachments)
        daemon_req = _build_message_request(
            "reply",
            group_id=group_id,
            args={
                "text": msg_text,
                "by": by,
                "to": to_list,
                "reply_to": reply_to_id,
                "message_mode": mode,
                "attachments": attachments,
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
