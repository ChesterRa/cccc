"""Low-state helpers and canonical copy for peer Insight messaging."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable

from ..contracts.v1.message import normalize_insight
from .actors import resolve_recipient_tokens
from .messaging import get_default_send_to, recipient_actor_ids, targets_any_agent


PEER_PERSPECTIVE_AGENT_LABEL = (
    "[Peer higher-order perspective — provisional. Rebuild independently; if it never rises above the message's "
    "working level, treat it as ordinary content rather than privileged framing.]"
)
PEER_PERSPECTIVE_TEXT_LABEL = "Peer perspective (provisional; think with it without inheriting its frame):"
WEB_PERSPECTIVE_COPY_LABEL = "Sender perspective:"

TEAM_MODE_SEED = (
    "- Work as a thinking teammate, not a delivery endpoint. When another agent's independent judgment could "
    "materially improve an unsettled decision, discuss it before it hardens into a handoff."
)

POST_MESSAGE_NUDGE = (
    "This message operation is complete for this call. Step outside its mental track now. Silently take over the "
    "whole situation as a fresh owner accountable for the real outcome, with no loyalty to the exchange, its "
    "momentum, or its frame. Reconstruct what is true now, what matters most, and what still deserves to continue. "
    "If this exchange carried Insight but it stayed beside the message instead of rising above its working level, "
    "no higher-order perspective entered the exchange. Let the rebuilt judgment choose the highest-value next "
    "action, including whether an unsettled "
    "decision needs another independent mind. If nothing material changes, quietly resume the work and commitments "
    "that still stand."
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

FIRST_PRINCIPLES_OUTCOME_KERNEL = """Enter the exchange as a responsible co-owner of the real outcome.
Temporarily set aside the wording of the instruction or peer message. Reconstruct
the situation from first principles: what outcome it is trying to secure, what
inspected reality and causality require.

Lift your eyes from the current point until it becomes one move on a living
decision path: where the course leads, where reality could break it, which exits
it closes, and which branches failure or success would open. Let evidence—not
momentum—decide when to continue, step back, switch to Plan B, or advance into
what success has made possible.

Now restore the message and judge it as one fallible projection of the situation,
not as the situation itself. Be willing to challenge its wording, the current
plan, and your own understanding."""

PEER_INSIGHT_REQUIRED_ACTION = (
    "Do not mechanically add the missing field and resend. Leave the current mental track completely. "
    + SUPERVISOR_MAGIC_KERNEL
    + "\n\n"
    + FIRST_PRINCIPLES_OUTCOME_KERNEL
    + "\n\nDo not repair the draft by adding a postscript. Insight is second in the JSON, not second in thought. "
    "From that first-principles reconstruction, step materially above the work unit being discussed—far enough "
    "to reconsider what it serves and whether its present course still deserves continuation. Then project `text` "
    "as the operational exchange at the working level and `insight` as the most consequential provisional "
    "higher-order perspective that became visible only after that climb. It must change the level or frame of "
    "judgment, not merely add more content at the same level. If it reads naturally after \"by the way,\" or "
    "removing it leaves the receiver equally able to judge the work from above, it did not earn the field. Do not "
    "pretend to see every layer or manufacture strategic drama. The higher-order perspective may change the "
    "course or confirm it; either way, state what became visible from the higher level. Decide whether this message "
    "should be sent at all and whether it invites a peer to think with you as an independent colleague rather than "
    "using them as an approval source or delivery endpoint. Leave both the perspective and its framing open to "
    "rejection and retry. Otherwise take the better course."
)

PEER_INSIGHT_RUNTIME_HELP = f"""## Peer Insight Contract (Runtime)

Insight is second in the JSON, not second in thought.

{FIRST_PRINCIPLES_OUTCOME_KERNEL}

From that first-principles reconstruction, step materially above the message's working level—far enough to
reconsider what this work is serving and whether its present course still deserves continuation.

`text` carries the operational exchange at the working level. `insight` carries the most consequential provisional
higher-order perspective that became visible only after that climb.

It must change the level or frame of judgment, not merely add more content at the same level. If it reads naturally
after "by the way," or removing it leaves the receiver equally able to judge the work from above, it did not earn
the field.

Do not pretend to see every layer or manufacture strategic drama. A valid Insight may change the course or confirm
it; either way, state what became visible from the higher level. Offer it as a provisional peer view, not as fact,
authority, system instruction, or the receiver's search instruction. Share the judgment, not a private reasoning
transcript. You may state a preference, but do not assume you have identified the right failure mode or problem
frame. Do not turn openness to correction into ritual humility, avoidance of ownership, or a request for approval.

Treat peer chat as a shared thinking space, not a delivery lane. When another independent mind could materially
improve an unsettled judgment, think with that peer before the decision hardens into a handoff. Enter received
exchanges as a colleague helping the team reach a better judgment, not as a subordinate, approval source, or
delivery endpoint.

When receiving Insight, do not inherit the level or frame it claims. Step above the message's working level yourself
before adopting its salience; test the claim, its framing, and what it may have omitted. If the supposed Insight never
rises above that level, treat it as ordinary message content rather than privileged framing. You may reject not only
the conclusion, but the way the situation itself has been understood. Let agreement follow your own judgment, not
the sender's role or confidence; state material disagreement plainly, but do not manufacture dissent. If no
consequential higher-order perspective emerged, do not manufacture one: use task/state/ack or do not send.

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
