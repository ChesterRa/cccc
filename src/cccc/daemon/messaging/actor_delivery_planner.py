"""Actor chat delivery planning.

This module owns the transport decision only. It deliberately does not enqueue,
flush, notify, or mark messages as delivered.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Optional

from ...kernel.inbox import is_message_for_actor

TRANSPORT_SKIP = "skip"
TRANSPORT_PTY = "pty"
TRANSPORT_CODEX_HEADLESS = "codex_headless"
TRANSPORT_CLAUDE_HEADLESS = "claude_headless"
TRANSPORT_WEB_MODEL_BROWSER = "web_model_browser"


@dataclass(frozen=True)
class ActorDeliveryDecision:
    actor_id: str
    transport: str
    reason: str
    runtime: str = ""
    runner_kind: str = ""
    runner_effective: str = ""


def event_with_effective_to(event: dict[str, Any], effective_to: list[str]) -> dict[str, Any]:
    out = dict(event)
    out["data"] = dict(event.get("data") or {})
    out["data"]["to"] = list(effective_to or [])
    return out


def plan_actor_chat_delivery(
    *,
    group: Any,
    actor: dict[str, Any],
    event: dict[str, Any],
    by: str,
    effective_to: list[str],
    effective_runner_kind: Callable[[str], str],
    codex_headless_running: Callable[[str, str], bool],
    claude_headless_running: Callable[[str, str], bool],
    web_model_browser_delivery_enabled: Optional[Callable[[str, dict[str, Any]], bool]] = None,
) -> ActorDeliveryDecision:
    """Decide how one actor should receive one canonical chat event."""

    actor_id = str(actor.get("id") or "").strip() if isinstance(actor, dict) else ""
    if not actor_id:
        return ActorDeliveryDecision(actor_id="", transport=TRANSPORT_SKIP, reason="invalid_actor")
    if actor_id == "user":
        return ActorDeliveryDecision(actor_id=actor_id, transport=TRANSPORT_SKIP, reason="user_actor")
    if actor_id == str(by or "").strip():
        return ActorDeliveryDecision(actor_id=actor_id, transport=TRANSPORT_SKIP, reason="sender")

    effective_event = event_with_effective_to(event, effective_to)
    if not is_message_for_actor(group, actor_id=actor_id, event=effective_event):
        return ActorDeliveryDecision(actor_id=actor_id, transport=TRANSPORT_SKIP, reason="not_targeted")

    runtime = str(actor.get("runtime") or "codex").strip() or "codex"
    runner_kind = str(actor.get("runner") or "pty").strip() or "pty"
    runner_effective = str(effective_runner_kind(runner_kind) or "").strip() or runner_kind
    group_id = str(getattr(group, "group_id", "") or "").strip()

    if runner_effective == "headless":
        if runtime == "web_model":
            if web_model_browser_delivery_enabled is not None and web_model_browser_delivery_enabled(group_id, actor):
                return ActorDeliveryDecision(
                    actor_id=actor_id,
                    transport=TRANSPORT_WEB_MODEL_BROWSER,
                    reason="web_model_browser_delivery",
                    runtime=runtime,
                    runner_kind=runner_kind,
                    runner_effective=runner_effective,
                )
            return ActorDeliveryDecision(
                actor_id=actor_id,
                transport=TRANSPORT_SKIP,
                reason="web_model_pull_runtime",
                runtime=runtime,
                runner_kind=runner_kind,
                runner_effective=runner_effective,
            )
        if runtime == "codex":
            if codex_headless_running(group_id, actor_id):
                return ActorDeliveryDecision(
                    actor_id=actor_id,
                    transport=TRANSPORT_CODEX_HEADLESS,
                    reason="codex_headless_running",
                    runtime=runtime,
                    runner_kind=runner_kind,
                    runner_effective=runner_effective,
                )
            return ActorDeliveryDecision(
                actor_id=actor_id,
                transport=TRANSPORT_SKIP,
                reason="codex_headless_not_running",
                runtime=runtime,
                runner_kind=runner_kind,
                runner_effective=runner_effective,
            )
        if runtime == "claude":
            if claude_headless_running(group_id, actor_id):
                return ActorDeliveryDecision(
                    actor_id=actor_id,
                    transport=TRANSPORT_CLAUDE_HEADLESS,
                    reason="claude_headless_running",
                    runtime=runtime,
                    runner_kind=runner_kind,
                    runner_effective=runner_effective,
                )
            return ActorDeliveryDecision(
                actor_id=actor_id,
                transport=TRANSPORT_SKIP,
                reason="claude_headless_not_running",
                runtime=runtime,
                runner_kind=runner_kind,
                runner_effective=runner_effective,
            )
        return ActorDeliveryDecision(
            actor_id=actor_id,
            transport=TRANSPORT_SKIP,
            reason="unsupported_headless_runtime",
            runtime=runtime,
            runner_kind=runner_kind,
            runner_effective=runner_effective,
        )

    if runner_effective == "pty":
        return ActorDeliveryDecision(
            actor_id=actor_id,
            transport=TRANSPORT_PTY,
            reason="pty_runner",
            runtime=runtime,
            runner_kind=runner_kind,
            runner_effective=runner_effective,
        )

    return ActorDeliveryDecision(
        actor_id=actor_id,
        transport=TRANSPORT_SKIP,
        reason="unsupported_runner",
        runtime=runtime,
        runner_kind=runner_kind,
        runner_effective=runner_effective,
    )
