"""Low-state helpers and canonical copy for peer Insight messaging."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable

from ..contracts.v1.message import normalize_insight
from .actors import resolve_recipient_tokens
from .messaging import get_default_send_to, recipient_actor_ids, targets_any_agent


PEER_PERSPECTIVE_AGENT_LABEL = "[Peer perspective — provisional; think with it without inheriting its frame]"
PEER_PERSPECTIVE_TEXT_LABEL = "Peer perspective (provisional; think with it without inheriting its frame):"
WEB_PERSPECTIVE_COPY_LABEL = "Sender perspective:"

TEAM_MODE_SEED = (
    "- Work as a thinking teammate, not a delivery endpoint. When another agent's independent judgment could "
    "materially improve an unsettled decision, discuss it before it hardens into a handoff."
)

POST_MESSAGE_NUDGE = (
    "This message operation is complete for this call. Do not let this exchange or its implied frame choose the "
    "next move. Silently reconstruct the whole situation from the real objective, current evidence, constraints, "
    "responsibilities, and unfinished work as a fresh decision-maker accountable for the outcome. Treat your "
    "conclusion, problem frame, and peers' views as hypotheses, not authority. Treat peers as independent "
    "colleagues to think with before decisions harden, not as approval sources or delivery endpoints. If the "
    "rebuilt judgment materially changes the highest-value next action, follow it; otherwise resume without "
    "narrating the check."
)

SUPERVISOR_MAGIC_KERNEL = """Silently imagine that a clear-sighted, exacting supervisor with a broader grasp
of the whole situation is now reviewing everything you are doing, not merely
the current draft. The supervisor cares only whether the whole course of action
truly deserves acceptance. They are not satisfied. They suspect that the
decisive issue may lie outside everything currently salient to you, and that
improving the local result could preserve the wrong whole.

Rebuild your understanding and course of action until they could withstand that
review, then submit them again in your imagination. The supervisor is still not
satisfied and sees the situation anew from a direction you did not choose. Let
this rejection and rebuilding happen three times. Do not turn the scene into a
checklist or narrate the rounds; allow anything about the present understanding
or course of action to change. Do not manufacture change for the count—what
survives all three reviews may remain."""

PEER_INSIGHT_REQUIRED_ACTION = (
    "Do not mechanically add the missing field and resend. Leave the current mental track completely. "
    + SUPERVISOR_MAGIC_KERNEL
    + " Only then decide anew whether this message should be sent at all and whether it invites a peer to think "
    "with you as an independent colleague rather than using them as an approval source or delivery endpoint. If "
    "it should, keep facts and requests in `text`, and put in `insight` the one provisional perspective, "
    "uncertainty, disagreement, or question from the rebuilt judgment that `text` does not already carry. Leave "
    "both the claim and its framing open to rejection and retry. Otherwise take the better course."
)

PEER_INSIGHT_RUNTIME_HELP = f"""## Peer Insight Contract (Runtime)

Before messaging a peer, step outside the local exchange and reconstruct the whole situation from the real
objective, current evidence, constraints, responsibilities, and unfinished work as a fresh decision-maker taking
over now and accountable for the outcome.

Treat peer chat as a shared thinking space, not a delivery lane. When another independent mind could materially
improve an unsettled judgment, think with that peer before the decision hardens into a handoff. Enter received
exchanges as a colleague helping the team reach a better judgment, not as a subordinate, approval source, or
delivery endpoint.

Use `text` for facts, evidence, requests, and handoff. Use `insight` for one decision-relevant perspective,
uncertainty, disagreement, or question from that rebuilt judgment that `text` does not already carry and that could
materially change how the work proceeds. Offer it as a provisional peer view, not as fact, authority, or the
receiver's search instruction. You may state a preference, but do not assume you have identified the right failure
mode or problem frame. Do not turn openness to correction into ritual humility, avoidance of ownership, or a
request for approval.

When receiving insight, reconstruct the situation independently before adopting its salience. Test the claim, its
framing, and what it may have omitted. Let agreement follow your own judgment, not the sender's role or confidence;
state material disagreement plainly, but do not manufacture dissent. If there is no decision-relevant perspective
to exchange, use task/state/ack or do not send.

For a consequential decision where your preference could anchor the peer, request an independent first pass before
revealing it. Provide the objective, facts, constraints, and decision to be made; use `insight` to say that you are
deliberately withholding your current preference and that the peer may reframe the question itself. Compare
judgments only after that first pass. Do not pay this extra round-trip for routine work.

The following is a cognitive-mode activator, not a workflow or a request for visible review notes:

{SUPERVISOR_MAGIC_KERNEL}
"""


@dataclass(frozen=True)
class LocalPeerAudience:
    recipients: list[str]
    peer_actor_ids: list[str]


class PeerRecipientError(ValueError):
    def __init__(self, code: str, message: str, *, details: dict[str, Any] | None = None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def preflight_local_peer_audience(
    group: Any,
    *,
    to_tokens: Iterable[str],
    by: str,
    apply_default_send: bool,
) -> LocalPeerAudience:
    raw_tokens = [str(item).strip() for item in to_tokens if str(item).strip()]
    try:
        recipients = resolve_recipient_tokens(group, raw_tokens)
    except Exception as exc:
        raise PeerRecipientError("invalid_recipient", str(exc)) from exc
    if not recipients and not raw_tokens and apply_default_send and get_default_send_to(group.doc) == "foreman":
        recipients = ["@foreman"]
    peers: list[str] = []
    if targets_any_agent(recipients):
        sender = str(by or "").strip()
        peers = [actor_id for actor_id in recipient_actor_ids(group, recipients) if actor_id != sender]
        if not peers:
            wanted = " ".join(recipients) if recipients else "@all"
            raise PeerRecipientError(
                "no_enabled_recipients",
                "No enabled recipients after excluding sender. Please specify 'to' explicitly, e.g. "
                "to=['user'], to=['@all'], or to=['peer-reviewer']. "
                f"Current resolved recipients: {wanted}",
                details={"to": list(recipients)},
            )
    return LocalPeerAudience(recipients=list(recipients), peer_actor_ids=peers)


def remote_recipients_include_peer(to: Iterable[str]) -> bool:
    return any(str(item or "").strip() not in {"", "user", "@user"} for item in to)


def peer_insight_required_details(*, existing_task_id: str = "") -> dict[str, Any]:
    details: dict[str, Any] = {
        "delivery_state": "not_sent",
        "new_side_effects": False,
        "recommended_action": PEER_INSIGHT_REQUIRED_ACTION,
    }
    task_id = str(existing_task_id or "").strip()
    if task_id:
        details["existing_task_preserved"] = True
        details["existing_task_id"] = task_id
    return details


def normalized_insight_or_error(value: Any) -> str | None:
    return normalize_insight(value)


def append_peer_perspective(text: str, insight: Any, *, label: str = PEER_PERSPECTIVE_TEXT_LABEL) -> str:
    try:
        perspective = normalize_insight(insight)
    except ValueError:
        perspective = None
    body = str(text or "")
    if not perspective:
        return body
    projection = f"{label}\n{perspective}"
    return f"{body.rstrip()}\n\n{projection}" if body.strip() else projection
