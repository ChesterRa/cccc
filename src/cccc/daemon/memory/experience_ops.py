from __future__ import annotations

from typing import Dict, Optional

from ...contracts.v1 import DaemonResponse
from .experience_governance import (
    handle_experience_governance,
    handle_experience_repair_memory,
    handle_procedural_skill_patch_governance,
    handle_procedural_skill_usage_report,
)
from .experience_promote import (
    handle_experience_promote_to_memory,
    handle_experience_runtime_prompt_delivery,
)


def try_handle_experience_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "experience_promote_to_memory":
        return handle_experience_promote_to_memory(args)
    if op == "experience_runtime_prompt_delivery":
        return handle_experience_runtime_prompt_delivery(args)
    if op == "experience_govern":
        return handle_experience_governance(args)
    if op == "experience_repair_memory":
        return handle_experience_repair_memory(args)
    if op == "procedural_skill_report_usage":
        return handle_procedural_skill_usage_report(args)
    if op == "procedural_skill_govern_patch":
        return handle_procedural_skill_patch_governance(args)
    return None
