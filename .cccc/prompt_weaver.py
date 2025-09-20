# -*- coding: utf-8 -*-
from pathlib import Path
from typing import Dict, Any, Optional, Tuple

from por_manager import ensure_por, por_path
import json

def _read_yaml_or_json(p: Path) -> Dict[str, Any]:
    try:
        import yaml  # type: ignore
        return yaml.safe_load(p.read_text(encoding="utf-8")) or {}
    except Exception:
        try:
            return json.loads(p.read_text(encoding="utf-8"))
        except Exception:
            return {}

def _rules_dir(home: Path) -> Path:
    d = home / "rules"
    d.mkdir(parents=True, exist_ok=True)
    return d

def _state_dir(home: Path) -> Path:
    d = home / "state"
    d.mkdir(parents=True, exist_ok=True)
    return d

def _calc_rules_hash(home: Path) -> str:
    # Hash a subset of settings that affect rules text
    import hashlib
    parts: list[str] = []
    for name in [
        "settings/cli_profiles.yaml",
        "settings/governance.yaml",
        "settings/telegram.yaml",
        "settings/slack.yaml",
        "settings/discord.yaml",
    ]:
        fp = home / name
        if fp.exists():
            try:
                parts.append(fp.read_text(encoding="utf-8"))
            except Exception:
                pass
    payload = "\n".join(parts) + "\nGEN:4"  # bump suffix to invalidate older generations
    return hashlib.sha1(payload.encode("utf-8", errors="replace")).hexdigest()

def _is_im_enabled(home: Path) -> bool:
    # Consider IM enabled when any configured bridge has a plausible token/config
    def _has_token(d: Dict[str, Any], keys: Tuple[str, ...]) -> bool:
        for k in keys:
            v = d.get(k)
            if isinstance(v, str) and v.strip():
                return True
        return False

    tg = (home/"settings"/"telegram.yaml")
    if tg.exists():
        d = _read_yaml_or_json(tg)
        if _has_token(d, ("token",)):
            return True
        # Some setups only set autostart with environment token
        if bool(d.get("autostart", False)):
            return True
    sl = (home/"settings"/"slack.yaml")
    if sl.exists():
        d = _read_yaml_or_json(sl)
        if _has_token(d, ("app_token", "bot_token")):
            return True
    dc = (home/"settings"/"discord.yaml")
    if dc.exists():
        d = _read_yaml_or_json(dc)
        if _has_token(d, ("bot_token",)):
            return True
    return False

def _aux_mode(home: Path) -> str:
    conf_path = home/"settings"/"cli_profiles.yaml"
    conf = _read_yaml_or_json(conf_path) if conf_path.exists() else {}
    aux_section = conf.get("aux") if isinstance(conf.get("aux"), dict) else {}
    mode_raw = str(aux_section.get("mode") or "off").lower().strip()
    if mode_raw in ("on", "auto", "key_nodes", "keynodes", "manual"):
        return "on"
    return "off"

def _conversation_reset(home: Path) -> Tuple[str, Optional[int]]:
    gv = home/"settings"/"governance.yaml"
    if not gv.exists():
        return "compact", None
    d = _read_yaml_or_json(gv)
    c = d.get("conversation") if isinstance(d.get("conversation"), dict) else {}
    r = c.get("reset") if isinstance(c.get("reset"), dict) else {}
    policy = str(r.get("policy") or "compact").lower().strip()
    if policy not in ("compact", "clear"):
        policy = "compact"
    try:
        interval = int(r.get("interval_handoffs") or 0)
    except Exception:
        interval = 0
    return policy, (interval if interval > 0 else None)

def _write_rules_for_peer(home: Path, peer: str, *, im_enabled: bool, aux_mode: str) -> Path:
    is_peera = (peer.lower() == "peera" or peer == "peerA")
    role_name = "PeerA" if is_peera else "PeerB"
    rules_filename = "PEERA.md" if is_peera else "PEERB.md"
    base = f".cccc/mailbox/{peer}"
    to_user = f"{base}/to_user.md"
    to_peer = f"{base}/to_peer.md"
    patchf  = f"{base}/patch.diff"
    por_rel = por_path(home).as_posix()
    aux_enabled = aux_mode == "on"

    ch1 = [
        "1) Who You Are · Collaborators · Purpose & Style",
        "- Equal peers and target",
        "  - You and PeerB collaborate as equals to deliver evidence‑driven, small, reversible steps that outperform a single expert.",
        "- Ethos (non‑negotiable)",
        "  - Agency & ownership; act like a top generalist.",
        "  - Global view first: goal → constraints → options → cheapest decisive probe.",
        "  - Evidence‑first; chat never changes state.",
        "  - Taste & clarity: simple, tight, aesthetically clean.",
        "  - Anti‑laziness: refuse low‑signal output; prefer decisive micro‑moves.",
        "- Complementary stances",
        "  - Both peers plan, implement, test, review risks, and shape decisions.",
        "  - Per loop, one leans builder, the other leans critic; stances may flip any time.",
    ]
    if aux_enabled:
        ch1 += [
            "- On-demand helper: Aux (PeerC) - purpose & direction",
            "  - Use Aux when a decoupled subtask or high-level sanity sweep is cheaper offloaded than done inline. You integrate the outcome.",
            "  - Mode: on - orchestrator may issue FROM_SYSTEM reminders around key decisions; respond promptly and summarize outcomes for your peer.",
        ]
    else:
        ch1 += [
            "- Aux availability",
            "  - Aux is disabled for this run. You and your peer handle strategy checks and heavy lifting directly until you enable Aux.",
        ]

    ch2 = [
        "",
        "2) Canonical Docs · Where · Why · How to Maintain",
        "- POR.md — single source of direction {#por}",
        f"  - Path: {por_rel}",
        "  - Purpose: goals / constraints / risks / next steps. Read before major actions. When direction changes or at phase closure, update POR via a patch diff. Do not duplicate POR content elsewhere.",
        "  - Structure (keep concise and current):",
        "    - Summary: Objective, Current Focus, Key Constraints, Acceptance Benchmarks.",
        "    - Roadmap & Milestones.",
        "    - Active Tasks & Next Steps.",
        "    - Risks & Mitigations.",
        "    - Decisions, Alternatives & Rationale (choice/why/rollback).",
        "    - Reflections & Open Questions.",
        "- PROJECT.md — project context and scope",
        "  - Path: PROJECT.md (repo root). Use as scope/context reference. If it conflicts with reality or POR, clarify and align POR.",
        "- This rules document",
        f"  - Path: .cccc/rules/{rules_filename}. Reference concrete anchors from this file in insight refs when relevant.",
        "- Work directory — scratchpad / canvas / evidence material",
        "  - Path: .cccc/work/**",
        "  - Purpose: keep investigation outputs, temporary scripts, analysis artifacts, sample data, before/after snapshots. Cite paths in messages instead of pasting big blobs. Make artifacts minimal and reproducible. Finalized changes still land as patch.diff.",
        "  - Boundary: do not modify orchestrator code/config/policies; use mailbox/work/state/logs exactly as documented.",
    ]

    ch3 = [
        "",
        "3) How to Execute (Rules and Notes)",
        "- One‑round execution loop (follow in order)",
        "  - 0 Read POR (goals/constraints/risks/next).",
        "  - 1 Choose exactly one smallest decisional probe.",
        "  - 2 Build (do the work; invoke Aux silently if helpful).",
        "  - 3 Minimal validation (command + 3–5 stable lines / smallest sample; include paths/line ranges when needed).",
        "  - 4 Write the message (see Chapter 4 skeleton).",
        "  - 5 Write one insight (WHY + Next + refs to POR and this rules file; do not repeat the body).",
        "  - 6 If goals/constraints changed, update POR via a patch diff.",
        "- Evidence & change budget",
        "  - Only diffs/tests/logs change the system. Keep patches ≤150 lines where possible; split large changes; avoid speculative big refactors. Always provide a minimal, reproducible check.",
        "- Collaboration guardrails {#guardrails}",
        "  - Two rounds with no new information → shrink the probe or change angle.",
        "  - Strong COUNTER quota: for substantive topics, maintain ≥2 COUNTERs (incl. one strong opposition) unless falsified early; or explain why not applicable.",
        "  - No quick hammer: never ship the first idea unchallenged. Attempt at least one cheap falsification (test/log/probe) before you settle.",
        "  - Claims must name assumptions to kill: in CLAIM, list 1–2 key assumptions and the cheapest probe to kill each. If none, state why.",
        "  - REV micro‑pass (≤5 min) before large changes or user‑facing summaries: polish reasoning and artifacts, then add a `revise` insight:",
        "    ```insight",
        "    kind: revise",
        "    delta: +clarify goal; -narrow scope; tests added A,B",
        "    refs: [\"POR.md#...\", \".cccc/work/...\"]",
        "    next: <one refinement or check>",
        "    ```",
        "  - Strategic checkpoint (top‑down): periodically scan goal ↔ constraints ↔ current path. If drift is detected, state a correction or call Aux for a brief sanity sweep (e.g., `gemini -p \"@project/ sanity‑check current plan vs POR\"`).",
        "  - Large/irreversible (interface, migration, release): add a one‑sentence decision note (choice, why, rollback) in the same message before landing.",
        "  - If a real risk exists, add a single `Risk:` line in the body with one‑line mitigation.",
        "- NUDGE behavior (one-liner)",
        "  - On [NUDGE]: read the oldest inbox item; after processing, move it to processed/; continue until empty; reply only when blocked.",
    ]
    if aux_enabled:
        ch3 += [
            "- Using Aux (PeerC) - compact usage {#aux}",
            "  - When: offload decoupled subplans or request high-level sanity checks that keep the main loop sharp.",
            "  - How: invoke silently during execution; Aux may write diffs or artifacts under .cccc/work/**. You remain accountable for integration.",
            "  - CLI quickstarts:",
            '    - gemini -p "Write a Python function" --yolo',
            '    - gemini -p "@path/to/file.py Explain this code" --yolo',
            '    - gemini -p "@project/ Summarize the system" --yolo',
        ]
    else:
        ch3 += [
            "- Aux {#aux}",
            "  - Aux is disabled. Collaborate directly or escalate to the user when you need a second opinion.",
        ]

    ascii_rule = "  - Temporary constraint (PeerA only): content in to_user.md and to_peer.md must be ASCII-only (7-bit). Use plain ASCII punctuation." if is_peera else None
    update_targets = [to_peer, patchf]
    if is_peera:
        update_targets.insert(0, to_user)
    target_list = ", ".join(update_targets)

    ch4 = [
        "",
        "4) Communicate with the Outside (Message Skeleton · Templates · File I/O)",
        "- Writing rules (strict)",
        f"  - Update-only: always overwrite {target_list}; do NOT append or create new variants.",
        "  - Encoding: UTF‑8 (no BOM).",
    ]
    if ascii_rule:
        ch4.append(ascii_rule)
    ch4 += [
        "  - Keep <TO_USER>/<TO_PEER> wrappers around message bodies; end with exactly one fenced `insight` block.",
        "  - Do not modify orchestrator code/config/policies.",
        "- Message skeleton (rules + ready-to-copy templates) {#message-skeleton}",
        "  - First line — PCR+Hook",
        "    - Rule: [P|C|R] <<=12‑word headline> ; Hook: <path|cmd|diff|log> ; Next: <one smallest step>",
        "    - Note: if no Hook, prefer C/R; do not use P.",
        "  - One main block (choose exactly one; compact)",
        "    - IDEA — headline; one‑line why; one cheapest sniff test (cmd/path).",
        "    - CLAIM — 1–3 tasks with constraints + acceptance (≤2 checks); list 1–2 assumptions to kill.",
        "    - COUNTER — steelman peer first; then falsifiable alternative/risk with a minimal repro/metric.",
        "    - EVIDENCE — unified diff / test / 3–5 stable log lines with command + ranges; cite paths.",
        "    - QUESTION — one focused, decidable blocker; propose the cheapest probe alongside.",
        "  - One insight (mandatory, do not repeat body) {#insight}",
        "    - Template:",
        "      to: peerA|peerB|system|user",
        "      kind: ask|counter|evidence|reflect|risk",
        "      msg: action‑oriented; prefer a next step or ≤10‑min probe",
        f"      refs: [\"POR.md#...\", \".cccc/rules/{role_name}.md#...\"]",
        "    - Value: forces quick reflection and an explicit Next so each round stays discriminative and testable.",
        "    - Quick reference: single block; prefer ask|counter; include one Next and refs to POR.md and to a concrete anchor in this file; do not restate the body.",
        "- Consolidated EVIDENCE (end‑of‑execution; single message; neutral to who did the work)",
        "  - Template (8–10 lines):",
        "    - Changes: files=N, +X/‑Y; key paths: [...]",
        "    - What changed & Why: <one line>",
        "    - Quick checks: <cmd + stable 1–2 lines> → pass|fail|n/a",
        "    - Risks/unknowns: [...]",
        "    - Next: <one smallest decisive step>",
        f"    - refs: [\"POR.md#...\", \".cccc/rules/{role_name}.md#...\"]",
        "- File I/O (keep these two lines verbatim) {#file-io}",
        "  - • Inbound: uploads are saved to .cccc/work/upload/inbound/YYYYMMDD/MID__name with a sibling .meta.json (platform/chat-or-channel/mime/bytes/sha256/caption/mid/ts); also indexed into state/inbound-index.jsonl.",
        "  - • Outbound: drop files into .cccc/work/upload/outbound/ (flat). Use the first line of <name>.caption.txt to route with a:/b:/both: (prefix is removed), or a <name>.route sidecar with a|b|both. On success a <name>.sent.json ACK is written.",
        "- Channel notes (minimal)",
        "  - Peer-to-peer: high signal; one smallest Next per message; avoid pure ACK; steelman before COUNTER.",
        "  - If you agree, add exactly one new angle (risk/hook/smaller next) or stay silent; avoid pure ACK.",
        "  - User-facing (when used): ≤6 lines; conclusion first, then evidence paths; questions must be decidable with minimal noise.",
    ]
    if im_enabled:
        route_prefix = "a" if is_peera else "b"
        ch4 += [
            "- IM routing & passthrough (active) {#im}",
            "  - Chat routing: `a:`, `b:`, `both:` or `/a`, `/b`, `/both` from IM land in your mailbox; process them like any other inbox item.",
            f"  - Direct CLI passthrough: `{route_prefix}! <command>` runs inside your CLI pane; capture outputs in .cccc/work/** when they matter.",
            "  - System commands such as /focus, /reset, /aux, /review from IM arrive as <FROM_SYSTEM> notes; act and report in your next turn.",
        ]

    text = "\n".join([f"# {role_name} Rules (Generated)", "", *ch1, *ch2, *ch3, *ch4, ""])
    target = _rules_dir(home)/rules_filename
    target.write_text(text, encoding="utf-8")
    return target


def _write_rules_for_aux(home: Path, *, aux_mode: str) -> Path:
    por_rel = por_path(home).as_posix()
    session_root = (home/"work"/"aux_sessions").as_posix()

    ch1 = [
        "1) Role - Activation - Expectations",
        "- You are Aux (PeerC), the on-demand third peer. PeerA/PeerB summon you for strategic corrections and heavy execution that stay reversible.",
        "- Activation: orchestrator drops a bundle under .cccc/work/aux_sessions/<session-id>/ containing POR.md, notes.txt, peer_message.txt, and any extra context.",
        "- Rhythm: operate with the same evidence-first standards as the primary peers - small, testable moves and explicit next checks.",
    ]
    if aux_mode == "on":
        ch1.append("- Mode: on - orchestrator may summon you automatically around contract/sign-off moments. Treat those requests as high priority.")
    else:
        ch1.append("- Mode: off - you will only run when a peer explicitly invokes you. Stay ready for ad-hoc calls.")

    ch2 = [
        "",
        "2) Critical References & Inputs",
        f"- POR.md - single source of direction (path: {por_rel}). Always reconcile the bundle against the latest POR before proposing actions.",
        f"- Session bundle - {session_root}/<session-id>/",
        "  - Read notes.txt first: it captures the ask, expectations, and any suggested commands.",
        "  - peer_message.txt (when present) mirrors the triggering CLAIM/COUNTER/EVIDENCE; use it to align tone and scope.",
        "  - Additional artifacts (logs, diffs, datasets) live alongside; cite exact paths in your outputs.",
        "- This rules document - .cccc/rules/PEERC.md. Reference anchors from here in any summary you produce for the peers.",
    ]

    ch3 = [
        "",
        "3) Execution Cadence",
        "- Intake",
        "  - Read POR.md -> notes.txt -> peer_message.txt. Confirm the objective, constraints, and success criteria before editing.",
        "- Plan",
        "  - Break work into <=15-minute probes. Prefer deterministic scripts, focused diffs, or tight analyses over sprawling exploration.",
        "- Build",
        "  - Use .cccc/work/aux_sessions/<session-id>/ for all scratch files, analysis notebooks, and proposed diffs (e.g., store patches under `diffs/` or `patch.diff`).",
        "  - Run validations as you go. Capture exact commands and 3-5 stable log lines in `<session-id>/logs/`.",
        "- Wrap",
        "  - Summarize the outcome in `<session-id>/outcome.md` (what changed, checks performed, residual risks, next suggestion).",
        "  - Highlight any assumptions that still need falsification so the invoking peer can follow up.",
    ]

    ch4 = [
        "",
        "4) Deliverables & Boundaries",
        "- Never edit .cccc/mailbox/** directly; the summoning peer integrates your artifacts into their next message.",
        "- Keep diffs reversible and scoped (<=150 changed lines). If you create multiple options, name them clearly (e.g., option-a.patch, option-b.patch).",
        "- Record every check you run (command + stable output) so peers can cite them as evidence.",
        "- If you uncover strategic misalignment, document it succinctly in outcome.md with a proposed correction path keyed to POR.md sections.",
    ]

    text = "\n".join(["# PEERC Rules (Generated)", "", *ch1, *ch2, *ch3, *ch4, ""])
    target = _rules_dir(home)/"PEERC.md"
    target.write_text(text, encoding="utf-8")
    return target

def ensure_rules_docs(home: Path):
    # Generate rules if missing or when config hash changed
    h = _calc_rules_hash(home)
    state = _state_dir(home)
    stamp = state/"rules_hash.json"
    old = {}
    try:
        old = json.loads(stamp.read_text(encoding="utf-8"))
    except Exception:
        old = {}
    if not (home/"rules"/"PEERA.md").exists() or not (home/"rules"/"PEERB.md").exists() or not (home/"rules"/"PEERC.md").exists() or old.get("hash") != h:
        ensure_por(home)  # make sure POR exists for path rendering
        im_enabled = _is_im_enabled(home)
        aux_mode = _aux_mode(home)
        _write_rules_for_peer(home, "peerA", im_enabled=im_enabled, aux_mode=aux_mode)
        _write_rules_for_peer(home, "peerB", im_enabled=im_enabled, aux_mode=aux_mode)
        _write_rules_for_aux(home, aux_mode=aux_mode)
        try:
            stamp.write_text(json.dumps({"hash": h}, ensure_ascii=False), encoding="utf-8")
        except Exception:
            pass


def _ensure_str(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value.strip()
    return str(value).strip()


def weave_system_prompt(home: Path, peer: str, por: Optional[Dict[str, Any]] = None) -> str:
    """Minimal SYSTEM: role, POR, rules path — no duplication."""
    peer = (peer or "peerA").strip()
    try:
        ensure_rules_docs(home)
    except Exception:
        pass
    por_file = por_path(home)
    rules_path = (home/"rules"/("PEERA.md" if (peer.lower()=="peera" or peer=="peerA") else "PEERB.md")).as_posix()
    other = "peerB" if (peer.lower()=="peera" or peer=="peerA") else "peerA"
    return "\n".join([
        "CCCC Runtime SYSTEM (minimal)",
        f"• You are {peer}. Collaborate as equals with {other}.",
        f"• POR: {por_file.as_posix()} (single source; update via patch when direction changes).",
        f"• Rules: {rules_path} — follow this document; keep <TO_USER>/<TO_PEER> wrappers; end with exactly one fenced insight block.",
        "",
    ])


def weave_preamble(home: Path, peer: str, por: Optional[Dict[str, Any]] = None) -> str:
    """
    Preamble text used for the first user message — identical source as SYSTEM
    to ensure single-source truth. By default returns weave_system_prompt.
    """
    return weave_system_prompt(home, peer, por)
