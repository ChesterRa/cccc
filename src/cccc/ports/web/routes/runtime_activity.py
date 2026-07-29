from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncIterator

from fastapi import APIRouter, Depends, HTTPException, Query, Request
from fastapi.responses import StreamingResponse
from starlette.concurrency import run_in_threadpool

from ....kernel.runtime_hooks.activity import project_snapshot, read_events
from ....kernel.runtime_hooks.contracts import RuntimeActivityEvent
from ..schemas import RouteContext, require_group


def create_routers(ctx: RouteContext) -> list[APIRouter]:
    router = APIRouter(
        prefix="/api/v1/groups/{group_id}/runtime-activity",
        dependencies=[Depends(require_group)],
    )

    @router.get("/snapshot")
    async def snapshot(group_id: str) -> dict[str, object]:
        try:
            events = await run_in_threadpool(
                read_events, ctx.home, group_id
            )
        except ValueError as exc:
            raise HTTPException(
                status_code=503,
                detail={
                    "code": "runtime_activity_unavailable",
                    "message": "runtime activity store is unavailable",
                    "details": {"group_id": group_id},
                },
            ) from exc
        projected = project_snapshot(events)
        return {
            "ok": True,
            "result": {
                "count": len(projected),
                "events": [event.to_dict() for event in projected],
            },
        }

    @router.get("/stream")
    async def stream(
        group_id: str,
        request: Request,
        replay: bool = Query(default=True),
    ) -> StreamingResponse:
        return StreamingResponse(
            _stream_events(ctx, group_id, request, replay=replay),
            media_type="text/event-stream",
            headers={"Cache-Control": "no-cache"},
        )

    return [router]


async def _stream_events(
    ctx: RouteContext,
    group_id: str,
    request: Request,
    *,
    replay: bool,
) -> AsyncIterator[bytes]:
    seen: set[str] = set()
    try:
        events = await run_in_threadpool(read_events, ctx.home, group_id)
    except ValueError:
        events = []
    if replay:
        for activity in project_snapshot(events):
            seen.add(activity.id)
            yield _sse_frame(activity)
    seen.update(event.id for event in events)
    polls = 0
    while not await request.is_disconnected():
        await asyncio.sleep(0.3)
        polls += 1
        try:
            events = await run_in_threadpool(read_events, ctx.home, group_id)
        except ValueError:
            continue
        for activity in events:
            if activity.id not in seen:
                seen.add(activity.id)
                yield _sse_frame(activity)
        for activity in project_snapshot(events):
            if activity.status == "stuck" and activity.id not in seen:
                seen.add(activity.id)
                yield _sse_frame(activity)
        if polls % 50 == 0:
            yield b": keep-alive\n\n"
        if len(seen) > 1024:
            current = {event.id for event in events}
            seen.intersection_update(
                current | {item for item in seen if item.startswith("stuck:")}
            )


def _sse_frame(activity: RuntimeActivityEvent) -> bytes:
    data = json.dumps(
        activity.to_dict(), ensure_ascii=False, separators=(",", ":")
    )
    return (
        f"event: runtime-activity\nid: {activity.id}\ndata: {data}\n\n"
    ).encode("utf-8")
