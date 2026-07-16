from __future__ import annotations

import json
from typing import Any, Dict

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import HTMLResponse, JSONResponse, Response
from pydantic import BaseModel, Field
from starlette.concurrency import run_in_threadpool

from ....kernel.nomcp_sessions import (
    NomcpSessionError,
    authorize_nomcp_session,
    create_nomcp_session,
    decode_b64url,
    get_nomcp_session,
    list_nomcp_sessions,
    nomcp_token_url,
    read_session_file,
    render_resources,
    render_session_home,
    render_text_page,
    revoke_nomcp_session,
    search_session,
    send_nomcp_advisory,
    session_diff,
    session_status,
)
from ....kernel.settings import resolve_remote_access_web_binding
from ..schemas import RouteContext, require_admin


class NomcpSessionCreateRequest(BaseModel):
    group_id: str
    title: str = ""
    brief: str = ""
    reply_to_event_id: str = ""
    recipient: str = "user"
    scope_key: str = ""
    allowed_paths: list[str] = Field(default_factory=list)
    expires_in_seconds: int = 24 * 60 * 60


def _error_response(exc: NomcpSessionError) -> JSONResponse:
    return JSONResponse(
        status_code=int(exc.status_code or 400),
        content={"ok": False, "error": {"code": exc.code, "message": exc.message, "details": {}}},
    )


def _raise_http(exc: NomcpSessionError) -> None:
    raise HTTPException(
        status_code=int(exc.status_code or 400),
        detail={"code": exc.code, "message": exc.message, "details": {}},
    )


def _base_url(request: Request) -> str:
    binding = resolve_remote_access_web_binding()
    public_url = str(binding.get("web_public_url") or "").strip()
    if public_url:
        base = public_url.rstrip("/")
        if base.endswith("/ui"):
            base = base[: -len("/ui")]
        return base.rstrip("/")
    return ""


def _with_url(request: Request, payload: Dict[str, Any], secret: str = "") -> Dict[str, Any]:
    sid = str(payload.get("sid") or "").strip()
    out = dict(payload)
    base_url = _base_url(request)
    if sid:
        out["session_url"] = f"{base_url}/nomcp/s/{sid}" if base_url else ""
    if sid and secret:
        out["session_url_with_token"] = nomcp_token_url(base_url, sid, secret) if base_url else ""
        out["secret_available"] = bool(base_url)
    else:
        out["secret_available"] = False
    return out


def _token(request: Request) -> str:
    return str(request.query_params.get("token") or "").strip()


def _format_is_markdown(request: Request) -> bool:
    return str(request.query_params.get("format") or "").strip().lower() in {"md", "markdown"}


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    router = APIRouter()

    @router.get("/api/v1/nomcp/sessions", dependencies=[Depends(require_admin)])
    async def nomcp_sessions_list(request: Request, group_id: str = "") -> Dict[str, Any]:
        sessions = await run_in_threadpool(list_nomcp_sessions, group_id)
        return {"ok": True, "result": {"sessions": [_with_url(request, item) for item in sessions]}}

    @router.post("/api/v1/nomcp/sessions", dependencies=[Depends(require_admin)])
    async def nomcp_sessions_create(req: NomcpSessionCreateRequest, request: Request) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(status_code=403, detail={"code": "read_only", "message": "No-MCP session creation is disabled in read-only mode.", "details": {}})
        if not _base_url(request):
            raise HTTPException(
                status_code=400,
                detail={"code": "public_url_required", "message": "Configure a public HTTPS Web URL before creating a No-MCP session.", "details": {}},
            )
        try:
            result = await run_in_threadpool(
                create_nomcp_session,
                group_id=req.group_id,
                title=req.title,
                brief=req.brief,
                reply_to_event_id=req.reply_to_event_id,
                recipient=req.recipient,
                scope_key=req.scope_key,
                allowed_paths=req.allowed_paths,
                expires_in_seconds=req.expires_in_seconds,
            )
        except NomcpSessionError as exc:
            _raise_http(exc)
        secret = str(result.pop("secret", "") or "")
        return {"ok": True, "result": {"session": _with_url(request, result, secret), "secret": secret}}

    @router.delete("/api/v1/nomcp/sessions/{sid}", dependencies=[Depends(require_admin)])
    async def nomcp_sessions_revoke(sid: str) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(status_code=403, detail={"code": "read_only", "message": "No-MCP session revoke is disabled in read-only mode.", "details": {}})
        revoked = await run_in_threadpool(revoke_nomcp_session, sid)
        if not revoked:
            raise HTTPException(status_code=404, detail={"code": "session_not_found", "message": "No-MCP session not found", "details": {}})
        return {"ok": True, "result": {"sid": sid, "revoked": True}}

    @router.get("/api/v1/nomcp/sessions/{sid}", dependencies=[Depends(require_admin)])
    async def nomcp_sessions_get(sid: str, request: Request) -> Dict[str, Any]:
        session = await run_in_threadpool(get_nomcp_session, sid)
        if not isinstance(session, dict):
            raise HTTPException(status_code=404, detail={"code": "session_not_found", "message": "No-MCP session not found", "details": {}})
        return {"ok": True, "result": {"session": _with_url(request, session)}}

    async def _authorized(request: Request, sid: str) -> Dict[str, Any]:
        try:
            return await run_in_threadpool(authorize_nomcp_session, sid, _token(request))
        except NomcpSessionError as exc:
            raise HTTPException(status_code=exc.status_code, detail={"code": exc.code, "message": exc.message, "details": {}}) from exc

    @router.get("/nomcp/s/{sid}")
    async def nomcp_home(sid: str, request: Request) -> Response:
        session = await _authorized(request, sid)
        markdown = _format_is_markdown(request)
        text = await run_in_threadpool(render_session_home, session, _token(request), markdown=markdown)
        media = "text/markdown; charset=utf-8" if markdown else "text/html; charset=utf-8"
        return Response(text, media_type=media)

    @router.get("/nomcp/s/{sid}/resources")
    async def nomcp_resources(sid: str, request: Request) -> Response:
        session = await _authorized(request, sid)
        markdown = _format_is_markdown(request)
        text = await run_in_threadpool(render_resources, session, _token(request), markdown=markdown)
        media = "text/markdown; charset=utf-8" if markdown else "text/html; charset=utf-8"
        return Response(text, media_type=media)

    @router.get("/nomcp/s/{sid}/status")
    async def nomcp_status(sid: str, request: Request, format: str = "") -> Response:
        session = await _authorized(request, sid)
        status = await run_in_threadpool(session_status, session)
        if str(format or "").strip().lower() == "json":
            return JSONResponse({"ok": True, "result": {"status": status}})
        body = json.dumps(status, ensure_ascii=False, indent=2)
        return HTMLResponse(render_text_page("No-MCP Status", body))

    @router.get("/nomcp/s/{sid}/read")
    async def nomcp_read(sid: str, request: Request, path: str, start: int = 1, end: int = 0, format: str = "") -> Response:
        session = await _authorized(request, sid)
        try:
            result = await run_in_threadpool(read_session_file, session, path, start=start, end=end)
        except NomcpSessionError as exc:
            return _error_response(exc)
        if str(format or "").strip().lower() == "json":
            return JSONResponse({"ok": True, "result": result})
        return HTMLResponse(render_text_page(f"No-MCP Read: {result.get('path')}", str(result.get("content") or "")))

    @router.get("/nomcp/s/{sid}/search")
    async def nomcp_search(sid: str, request: Request, q: str, format: str = "") -> Response:
        session = await _authorized(request, sid)
        try:
            result = await run_in_threadpool(search_session, session, q)
        except NomcpSessionError as exc:
            return _error_response(exc)
        if str(format or "").strip().lower() == "json":
            return JSONResponse({"ok": True, "result": result})
        body = json.dumps(result, ensure_ascii=False, indent=2)
        return HTMLResponse(render_text_page("No-MCP Search", body))

    @router.get("/nomcp/s/{sid}/diff")
    async def nomcp_diff(sid: str, request: Request, path: str = "", format: str = "") -> Response:
        session = await _authorized(request, sid)
        try:
            result = await run_in_threadpool(session_diff, session, path)
        except NomcpSessionError as exc:
            return _error_response(exc)
        if str(format or "").strip().lower() == "json":
            return JSONResponse({"ok": True, "result": result})
        body = "\n".join(
            part for part in (
                "## Diff stat\n" + str(result.get("stat") or ""),
                "## Name status\n" + str(result.get("name_status") or ""),
                "## Diff\n" + str(result.get("diff") or ""),
            ) if part.strip()
        )
        return HTMLResponse(render_text_page("No-MCP Diff", body))

    @router.post("/nomcp/s/{sid}/send")
    async def nomcp_send_post(sid: str, request: Request) -> Response:
        token = _token(request)
        content_type = str(request.headers.get("content-type") or "").lower()
        payload: Dict[str, Any] = {}
        if "application/json" in content_type:
            try:
                raw = await request.json()
            except Exception:
                raw = {}
            payload = raw if isinstance(raw, dict) else {}
        else:
            form = await request.form()
            payload = {str(k): v for k, v in form.items()}
        try:
            result = await run_in_threadpool(
                send_nomcp_advisory,
                sid,
                token,
                msg_id=str(payload.get("msg_id") or ""),
                text=str(payload.get("text") or ""),
                title=str(payload.get("title") or ""),
                via="post",
            )
        except NomcpSessionError as exc:
            return _error_response(exc)
        return HTMLResponse(render_text_page("No-MCP Advisory", json.dumps(result, ensure_ascii=False, indent=2)))

    @router.get("/nomcp/s/{sid}/send")
    async def nomcp_send_get(sid: str, request: Request, msg_id: str = "", text_b64url: str = "", title: str = "") -> Response:
        try:
            text = decode_b64url(text_b64url)
            result = await run_in_threadpool(
                send_nomcp_advisory,
                sid,
                _token(request),
                msg_id=msg_id,
                text=text,
                title=title,
                via="get",
            )
        except NomcpSessionError as exc:
            return _error_response(exc)
        return HTMLResponse(render_text_page("No-MCP Advisory", json.dumps(result, ensure_ascii=False, indent=2)))

    return [router]
