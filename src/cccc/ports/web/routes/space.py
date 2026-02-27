from __future__ import annotations

from typing import Any, Dict

from fastapi import FastAPI, HTTPException

from ..schemas import (
    GroupSpaceArtifactActionRequest,
    GroupSpaceBindRequest,
    GroupSpaceIngestRequest,
    GroupSpaceJobActionRequest,
    GroupSpaceProviderAuthRequest,
    GroupSpaceProviderCredentialUpdateRequest,
    GroupSpaceQueryRequest,
    GroupSpaceSourceActionRequest,
    GroupSpaceSyncRequest,
    RouteContext,
)


def register_space_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    @app.get("/api/v1/groups/{group_id}/space/status")
    async def group_space_status(group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_status",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                },
            }
        )

    @app.get("/api/v1/groups/{group_id}/space/spaces")
    async def group_space_spaces(group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_spaces",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/space/bind")
    async def group_space_bind(group_id: str, req: GroupSpaceBindRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_bind",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "action": str(req.action or "bind").strip() or "bind",
                    "remote_space_id": str(req.remote_space_id or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/space/ingest")
    async def group_space_ingest(group_id: str, req: GroupSpaceIngestRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_ingest",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "kind": str(req.kind or "context_sync").strip() or "context_sync",
                    "payload": dict(req.payload or {}),
                    "idempotency_key": str(req.idempotency_key or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/space/query")
    async def group_space_query(group_id: str, req: GroupSpaceQueryRequest) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_query",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "query": str(req.query or "").strip(),
                    "options": dict(req.options or {}),
                },
            }
        )

    @app.get("/api/v1/groups/{group_id}/space/sources")
    async def group_space_sources_list(group_id: str, provider: str = "notebooklm") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_sources",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "action": "list",
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/space/sources")
    async def group_space_sources_action(group_id: str, req: GroupSpaceSourceActionRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_sources",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "action": str(req.action or "refresh").strip() or "refresh",
                    "source_id": str(req.source_id or "").strip(),
                    "new_title": str(req.new_title or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @app.get("/api/v1/groups/{group_id}/space/artifacts")
    async def group_space_artifacts_list(
        group_id: str,
        provider: str = "notebooklm",
        kind: str = "",
    ) -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_artifact",
                "args": {
                    "group_id": group_id,
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "action": "list",
                    "kind": str(kind or "").strip().lower(),
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/space/artifacts")
    async def group_space_artifacts_action(group_id: str, req: GroupSpaceArtifactActionRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_artifact",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "action": str(req.action or "generate").strip() or "generate",
                    "kind": str(req.kind or "").strip().lower(),
                    "options": dict(req.options or {}),
                    "wait": bool(req.wait),
                    "save_to_space": bool(req.save_to_space),
                    "output_path": str(req.output_path or "").strip(),
                    "output_format": str(req.output_format or "").strip().lower(),
                    "artifact_id": str(req.artifact_id or "").strip(),
                    "timeout_seconds": float(req.timeout_seconds),
                    "initial_interval": float(req.initial_interval),
                    "max_interval": float(req.max_interval),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @app.get("/api/v1/groups/{group_id}/space/jobs")
    async def group_space_jobs_list(
        group_id: str,
        provider: str = "notebooklm",
        state: str = "",
        limit: int = 50,
    ) -> Dict[str, Any]:
        args: Dict[str, Any] = {
            "group_id": group_id,
            "provider": str(provider or "notebooklm").strip() or "notebooklm",
            "action": "list",
            "limit": int(limit or 50),
        }
        state_value = str(state or "").strip()
        if state_value:
            args["state"] = state_value
        return await ctx.daemon({"op": "group_space_jobs", "args": args})

    @app.post("/api/v1/groups/{group_id}/space/jobs")
    async def group_space_jobs_action(group_id: str, req: GroupSpaceJobActionRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_jobs",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "action": str(req.action or "retry").strip() or "retry",
                    "job_id": str(req.job_id or "").strip(),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @app.post("/api/v1/groups/{group_id}/space/sync")
    async def group_space_sync(group_id: str, req: GroupSpaceSyncRequest) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Group Space write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_sync",
                "args": {
                    "group_id": group_id,
                    "provider": str(req.provider or "notebooklm").strip() or "notebooklm",
                    "action": str(req.action or "run").strip() or "run",
                    "force": bool(req.force),
                    "by": str(req.by or "user").strip() or "user",
                },
            }
        )

    @app.get("/api/v1/space/providers/{provider}/credential")
    async def group_space_provider_credential_status(provider: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_provider_credential_status",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(by or "user").strip() or "user",
                },
            }
        )

    @app.post("/api/v1/space/providers/{provider}/credential")
    async def group_space_provider_credential_update(
        provider: str,
        req: GroupSpaceProviderCredentialUpdateRequest,
    ) -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Provider credential write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_provider_credential_update",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(req.by or "user").strip() or "user",
                    "auth_json": str(req.auth_json or ""),
                    "clear": bool(req.clear),
                },
            }
        )

    @app.post("/api/v1/space/providers/{provider}/health")
    async def group_space_provider_health_check(provider: str, by: str = "user") -> Dict[str, Any]:
        if ctx.read_only:
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Provider health-check endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_provider_health_check",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(by or "user").strip() or "user",
                },
            }
        )

    @app.post("/api/v1/space/providers/{provider}/auth")
    async def group_space_provider_auth(provider: str, req: GroupSpaceProviderAuthRequest) -> Dict[str, Any]:
        action = str(req.action or "status").strip() or "status"
        if ctx.read_only and action != "status":
            raise HTTPException(
                status_code=403,
                detail={
                    "code": "read_only",
                    "message": "Provider auth-flow write endpoints are disabled in read-only (exhibit) mode.",
                },
            )
        return await ctx.daemon(
            {
                "op": "group_space_provider_auth",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(req.by or "user").strip() or "user",
                    "action": action,
                    "timeout_seconds": int(req.timeout_seconds or 900),
                },
            }
        )

    @app.get("/api/v1/space/providers/{provider}/auth")
    async def group_space_provider_auth_status(provider: str, by: str = "user") -> Dict[str, Any]:
        return await ctx.daemon(
            {
                "op": "group_space_provider_auth",
                "args": {
                    "provider": str(provider or "notebooklm").strip() or "notebooklm",
                    "by": str(by or "user").strip() or "user",
                    "action": "status",
                },
            }
        )
