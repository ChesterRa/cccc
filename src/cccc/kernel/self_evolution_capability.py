"""Built-in CCCC self-evolution skill metadata."""

from __future__ import annotations

from importlib.resources import files


SELF_EVOLUTION_CAPABILITY_ID = "skill:cccc:self-evolution"
LEGACY_SELF_EVOLUTION_CAPABILITY_ID = "skill:agent_self_proposed:cccc-self-evolution"
DEFAULT_GROUP_CAPABILITY_SEED_VERSION = 2


def _capsule_text() -> str:
    return (
        files("cccc.resources")
        .joinpath("cccc-self-evolution.md")
        .read_text(encoding="utf-8")
        .strip()
    )


SELF_EVOLUTION_CAPABILITY_RECORD = {
    "name": "cccc-self-evolution",
    "description_short": (
        "Review complete visible CCCC group history and propose confirmed improvements at the prompt, "
        "structured-context, workflow, Harness, or optimizer level."
    ),
    "use_when": (
        "The user asks CCCC to learn from collaboration, review recurring mistakes, or improve itself.",
        "The user invokes /cccc-self-evolution or controls the self-evolution capability.",
    ),
    "avoid_when": (
        "The request is a one-off task with no reusable improvement.",
        "The capability is disabled or the user paused the current run.",
    ),
    "gotchas": (
        "Invocation is not write authorization; apply only after direct confirmation of the current proposal.",
        "Choose the target by semantic ownership, not file extension or a shallow-to-deep trial sequence.",
    ),
    "evidence_kind": "confirmed proposal, exact target and scope, validation result, and rollback evidence",
    "capsule_text": _capsule_text(),
    "tags": (
        "self-evolution",
        "learning",
        "workflow",
        "harness",
        "optimizer",
        "cccc-glue",
    ),
}
