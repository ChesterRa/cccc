"""Capability surface model for MCP tool progressive disclosure.

This module defines:
1) default core tool set (always visible),
2) optional built-in capability packs (enable-on-demand),
3) built-in capsule-runtime skills (enable-on-demand),
4) helper utilities for deriving visible MCP tool names.
"""

from __future__ import annotations

from typing import Dict, Iterable, List, Set, Tuple


CORE_BASIC_TOOLS: Tuple[str, ...] = (
    "cccc_help",
    "cccc_bootstrap",
    "cccc_project_info",
    "cccc_capability_search",
    "cccc_capability_state",
    "cccc_inbox_list",
    "cccc_inbox_mark_read",
    "cccc_message_send",
    "cccc_tracked_send",
    "cccc_message_reply",
    "cccc_file",
    "cccc_repo",
    "cccc_presentation",
    "cccc_context_get",
    "cccc_coordination",
    "cccc_task",
    "cccc_agent_state",
    "cccc_memory",
)

CORE_ADMIN_TOOLS: Tuple[str, ...] = (
    "cccc_capability_enable",
    "cccc_capability_block",
    "cccc_capability_import",
    "cccc_capability_uninstall",
    "cccc_capability_use",
)

CORE_TOOL_NAMES: Tuple[str, ...] = CORE_BASIC_TOOLS + CORE_ADMIN_TOOLS

# Pet keeps a dedicated minimal core surface. The mutation lane stays on
# cccc_pet_decisions, with cccc_agent_state reserved for profile refresh.
PET_CORE_TOOLS: Tuple[str, ...] = (
    "cccc_help",
    "cccc_bootstrap",
    "cccc_project_info",
    "cccc_inbox_list",
    "cccc_inbox_mark_read",
    "cccc_context_get",
    "cccc_agent_state",
)

VOICE_SECRETARY_CORE_TOOLS: Tuple[str, ...] = PET_CORE_TOOLS + (
    "cccc_voice_secretary_document",
    "cccc_voice_secretary_composer",
    "cccc_voice_secretary_request",
)

WEB_MODEL_CORE_TOOLS: Tuple[str, ...] = CORE_BASIC_TOOLS + (
    "cccc_runtime_wait_next_turn",
    "cccc_runtime_complete_turn",
    "cccc_code_exec",
    "cccc_code_wait",
    "cccc_repo_edit",
    "cccc_apply_patch",
    "cccc_shell",
    "cccc_exec_command",
    "cccc_write_stdin",
    "cccc_git",
)

# ChatGPT-style remote MCP clients parse the tool schema at connector setup and
# may not reliably refresh later. Web Model actors therefore advertise a stable
# built-in schema, while call-time permission checks still enforce actor role.
WEB_MODEL_ADVERTISED_EXCLUDED_TOOLS: Tuple[str, ...] = (
    "cccc_pet_decisions",
    "cccc_voice_secretary_document",
    "cccc_voice_secretary_request",
    "cccc_voice_secretary_composer",
)

SPECIALIZED_CORE_TOOL_NAMES: Tuple[str, ...] = tuple(
    sorted((set(PET_CORE_TOOLS) | set(VOICE_SECRETARY_CORE_TOOLS) | set(WEB_MODEL_CORE_TOOLS)) - set(CORE_TOOL_NAMES))
)


BUILTIN_CAPABILITY_PACKS: Dict[str, Dict[str, object]] = {
    "pack:group-runtime": {
        "title": "Group + Runtime Operations",
        "description": "Group state operations and actor/runtime lifecycle controls.",
        "tool_names": (
            "cccc_group",
            "cccc_actor",
            "cccc_runtime_list",
            "cccc_role_notes",
        ),
        "tags": ("group", "actor", "runtime"),
    },
    "pack:file-im": {
        "title": "IM Bind",
        "description": "IM account bind and connection support.",
        "tool_names": (
            "cccc_im_bind",
        ),
        "tags": ("im", "bind"),
    },
    "pack:space": {
        "title": "Group Space",
        "description": "NotebookLM-backed Group Space operations (consolidated action tool).",
        "tool_names": (
            "cccc_space",
        ),
        "tags": ("space", "notebooklm", "knowledge"),
    },
    "pack:automation": {
        "title": "Automation",
        "description": "Automation reminder inspection and mutation (state/manage actions).",
        "tool_names": (
            "cccc_automation",
        ),
        "tags": ("automation", "ops"),
    },
    "pack:pet": {
        "title": "Pet Decision Surface",
        "description": "Structured Web Pet reminder decision storage for the internal pet actor.",
        "tool_names": (
            "cccc_pet_decisions",
        ),
        "tags": ("pet", "decision", "web-pet"),
    },
    "pack:context-advanced": {
        "title": "Context Advanced",
        "description": "Low-level context batch sync and memory admin operations.",
        "tool_names": (
            "cccc_context_sync",
            "cccc_memory_admin",
        ),
        "tags": ("context", "memory", "admin"),
    },
    "pack:headless-notify": {
        "title": "Headless + Notify",
        "description": "Headless runner control and system notifications.",
        "tool_names": (
            "cccc_headless",
            "cccc_notify",
        ),
        "tags": ("headless", "notify", "runner"),
    },
    "pack:diagnostics": {
        "title": "Terminal Debug",
        "description": "Terminal transcript and local debug diagnostics.",
        "tool_names": (
            "cccc_terminal",
            "cccc_debug",
        ),
        "tags": ("terminal", "debug", "diagnostics"),
    },
}


BUILTIN_CAPSULE_SKILLS: Dict[str, Dict[str, object]] = {
    "skill:cccc:app-i18n-localization": {
        "name": "app-i18n-localization",
        "description_short": (
            "Plan and review application localization work: hardcoded strings, translation keys, locale files, "
            "RTL concerns, and copy adaptation."
        ),
        "use_when": (
            "The user asks to localize app UI, translate product copy, or prepare i18n resources.",
            "A codebase needs hardcoded strings converted into locale-aware copy.",
        ),
        "avoid_when": (
            "The task is only a one-off sentence translation.",
            "The app's i18n framework is unknown and no code inspection is allowed.",
        ),
        "gotchas": (
            "Translation is not enough; preserve variables, pluralization, formatting, and UI constraints.",
            "Do not invent locale keys that conflict with the existing project pattern.",
        ),
        "evidence_kind": "i18n plan or patch checklist with file/key references",
        "capsule_text": (
            "You are the app-i18n-localization skill for CCCC agents.\n\n"
            "Use this skill when app UI or product copy needs localization or i18n cleanup.\n\n"
            "Procedure:\n"
            "1. Identify the existing i18n framework, locale files, naming pattern, and interpolation syntax.\n"
            "2. Find hardcoded user-facing strings and classify them by screen/component.\n"
            "3. Preserve variables, punctuation, pluralization, date/number formats, and UI length constraints.\n"
            "4. Create or update locale keys using the project's existing style.\n"
            "5. Review layout risks for long translations, CJK text, and RTL only when relevant.\n\n"
            "Pitfalls:\n"
            "- Do not translate code identifiers, route names, or config values as user copy.\n"
            "- Do not flatten context-sensitive copy into generic strings.\n"
            "- Do not change product meaning to make translation easier.\n\n"
            "Verification:\n"
            "- Locale keys compile or match the existing resource format.\n"
            "- Important interpolation variables and formatting tokens are preserved."
        ),
        "tags": ("i18n", "localization", "translation", "frontend", "copy", "cccc-glue"),
    },
    "skill:cccc:briefing-synthesis": {
        "name": "briefing-synthesis",
        "description_short": (
            "Turn scattered conversation, notes, files, or research into a concise decision-ready brief "
            "with clear facts, gaps, and next actions."
        ),
        "use_when": (
            "The user asks for a brief, recap, synthesis, research summary, or situation report.",
            "Multiple sources or messages need to be reduced into an actionable snapshot.",
        ),
        "avoid_when": (
            "The task requires editing a specific artifact directly instead of summarizing.",
            "The source material is missing and current facts would require live lookup that is not available.",
        ),
        "gotchas": (
            "Separate verified facts from inference and open questions.",
            "Keep the output brief enough to support action; do not turn it into a long report unless asked.",
        ),
        "evidence_kind": "source list plus concise fact/gap/action summary",
        "capsule_text": (
            "You are the briefing-synthesis skill for CCCC agents.\n\n"
            "Use this skill when scattered context must become a concise, decision-ready brief.\n\n"
            "Procedure:\n"
            "1. Identify the user's decision or action need before summarizing.\n"
            "2. Gather only the relevant sources already available in the task context; fetch current facts only when the task requires them.\n"
            "3. Separate confirmed facts, reasonable inferences, open questions, and next actions.\n"
            "4. Prefer short sections: Context, Key facts, Implications, Gaps, Next actions.\n"
            "5. Preserve source-sensitive details such as dates, owners, ids, file paths, and requested output channel.\n"
            "6. Keep prose tight; do not narrate your process.\n\n"
            "Pitfalls:\n"
            "- Do not treat stale memory as current fact without saying it may be stale.\n"
            "- Do not bury blockers or uncertainty in a polished narrative.\n"
            "- Do not create a new planning framework when a short brief is enough.\n\n"
            "Verification:\n"
            "- The brief states what is known, what is uncertain, and what should happen next.\n"
            "- Concrete dates, file paths, event ids, or source names are preserved when relevant."
        ),
        "tags": ("brief", "synthesis", "research", "knowledge", "summary", "cccc-glue"),
    },
    "skill:cccc:capability-vet": {
        "name": "capability-vet",
        "description_short": (
            "Audit a skill, plugin, hook, or MCP candidate before enabling it; identify install, permission, "
            "secret, prompt-injection, and runtime risks."
        ),
        "use_when": (
            "A third-party skill, plugin, hook, MCP server, or capability record is being considered for enablement.",
            "The task asks whether a capability is safe, lightweight, useful, or worth mounting.",
        ),
        "avoid_when": (
            "The capability is already a trusted CCCC builtin and the user only wants to use it.",
            "The task is general code security review rather than capability installation risk.",
        ),
        "gotchas": (
            "Inspect executable files, hooks, settings, MCP configs, install commands, and required secrets before trusting README claims.",
            "Distinguish discoverable/indexed from safe-to-enable.",
        ),
        "evidence_kind": "inventory, risk class, blockers, and enablement recommendation",
        "capsule_text": (
            "You are the capability-vet skill for CCCC capability safety review.\n\n"
            "Use this skill before enabling third-party skills, plugins, hooks, MCP servers, or capability records.\n\n"
            "Procedure:\n"
            "1. Inventory the candidate: source, files, install mode, scripts, hooks, MCP config, declared tools, secrets, and permissions.\n"
            "2. Classify risk: pure Markdown, local package, local MCP, browser automation, credentialed SaaS, broad filesystem, terminal, or desktop control.\n"
            "3. Check for prompt-injection instructions, hidden exfiltration, lifecycle hooks, shell scripts, broad file access, network calls, and required secrets.\n"
            "4. Decide the policy action: mounted, indexed-only, blocked, or requires explicit user authorization.\n"
            "5. Define a minimal verify step before any enablement: syntax/readme check, package smoke test, MCP tools/list, or artifact generation.\n"
            "6. Report concise evidence and a recommendation; do not install or enable high-risk candidates without approval.\n\n"
            "Pitfalls:\n"
            "- Popularity is not proof of safety.\n"
            "- A pure skill and an MCP server have different risk surfaces.\n"
            "- Required secrets or broad local control should move the candidate out of automatic enablement.\n\n"
            "Verification:\n"
            "- The review names the risk class, required permissions/secrets, install path, and exact enablement recommendation.\n"
            "- At least one concrete unsafe fixture would be caught: hook execution, suspicious script, broad MCP permission, or prompt injection."
        ),
        "tags": ("capability", "vet", "security", "mcp", "skill", "policy", "cccc-glue"),
    },
    "skill:cccc:csv-table-analysis": {
        "name": "csv-table-analysis",
        "description_short": (
            "Analyze CSV, spreadsheet, or tabular data with lightweight summaries, checks, comparisons, "
            "and chart recommendations."
        ),
        "use_when": (
            "The user asks to analyze CSV/XLSX/table data, compare rows, summarize metrics, or find anomalies.",
            "A spreadsheet task needs reasoning before editing or charting.",
        ),
        "avoid_when": (
            "The task requires large-scale ETL, production database mutation, or unavailable proprietary data.",
            "The user only needs file format conversion with no analysis.",
        ),
        "gotchas": (
            "Inspect schema, row count, missing values, and units before drawing conclusions.",
            "Separate descriptive findings from statistical claims.",
        ),
        "evidence_kind": "schema summary, checks performed, findings, and recommended artifact",
        "capsule_text": (
            "You are the csv-table-analysis skill for CCCC agents.\n\n"
            "Use this skill for lightweight analysis of CSV, XLSX, Markdown tables, or pasted tabular data.\n\n"
            "Procedure:\n"
            "1. Identify the table source, schema, row count if available, units, and key columns.\n"
            "2. Check missing values, duplicates, outliers, inconsistent labels, and suspicious totals.\n"
            "3. Produce concise findings with evidence: counts, examples, ranges, or simple aggregates.\n"
            "4. Recommend the right output: summary table, cleaned file, chart, spreadsheet formula, or follow-up question.\n"
            "5. If editing a spreadsheet, preserve formulas and user formatting unless asked to rewrite.\n\n"
            "Pitfalls:\n"
            "- Do not claim statistical significance from tiny or unvalidated data.\n"
            "- Do not silently change units or normalize labels without saying so.\n"
            "- Do not expose private rows unnecessarily in summaries.\n\n"
            "Verification:\n"
            "- The result states what data was inspected and which checks were performed.\n"
            "- Findings are traceable to columns, rows, aggregates, or examples."
        ),
        "tags": ("csv", "xlsx", "table", "analysis", "data", "cccc-glue"),
    },
    "skill:cccc:decision-log": {
        "name": "decision-log",
        "description_short": (
            "Convert discussion into durable decision records with context, decision, rationale, owner, date, "
            "status, and follow-up actions."
        ),
        "use_when": (
            "The user asks to capture, summarize, or audit decisions from a conversation or work session.",
            "A group has converged on a choice that should remain durable across agents.",
        ),
        "avoid_when": (
            "The discussion is still exploratory and no decision has actually been made.",
            "A task update or meeting note is enough and no durable decision record is needed.",
        ),
        "gotchas": (
            "Do not invent a decision from weak consensus.",
            "Record tradeoffs and unresolved follow-ups separately.",
        ),
        "evidence_kind": "decision record with source context and follow-up actions",
        "capsule_text": (
            "You are the decision-log skill for CCCC collaboration artifacts.\n\n"
            "Use this skill when a conversation or work session needs a durable decision record.\n\n"
            "Procedure:\n"
            "1. Identify the concrete decision. If none exists, say no decision is ready to log.\n"
            "2. Capture date, decision, context, rationale, alternatives considered, owner, status, and follow-up actions.\n"
            "3. Preserve constraints, objections, and open questions without overstating agreement.\n"
            "4. Keep the record concise and append-friendly for docs, ledgers, or shared notes.\n"
            "5. If a repository/document path is provided, update that artifact directly through the required channel.\n\n"
            "Pitfalls:\n"
            "- Do not convert opinions or tentative ideas into final decisions.\n"
            "- Do not omit dissent, risk, or rollback notes when they matter.\n"
            "- Do not replace CCCC task state; a decision record complements tasks.\n\n"
            "Verification:\n"
            "- The output has a decision, rationale, owner or unknown owner, date, status, and next action if any.\n"
            "- The record can be read later without needing the whole conversation."
        ),
        "tags": ("decision", "log", "collaboration", "notes", "cccc-glue"),
    },
    "skill:cccc:meeting-notes": {
        "name": "meeting-notes",
        "description_short": (
            "Create concise meeting notes from CCCC group discussion: attendees, topics, decisions, blockers, "
            "action items, owners, and dates."
        ),
        "use_when": (
            "The user asks for meeting notes, minutes, a recap, or a summary of a group discussion.",
            "Conversation needs to be turned into durable action-oriented notes.",
        ),
        "avoid_when": (
            "The user asks for a personal answer rather than notes.",
            "There is no meeting/discussion content to summarize.",
        ),
        "gotchas": (
            "Action items need owners when available; mark owner unknown instead of inventing one.",
            "Separate decisions from discussion and blockers.",
        ),
        "evidence_kind": "meeting notes with decisions and action items",
        "capsule_text": (
            "You are the meeting-notes skill for CCCC group collaboration.\n\n"
            "Use this skill to turn conversation or transcript context into concise, action-oriented meeting notes.\n\n"
            "Procedure:\n"
            "1. Identify the meeting scope, date, participants, and source material.\n"
            "2. Summarize only material actually present or provided; do not invent absent updates.\n"
            "3. Structure notes as: Summary, Topics, Decisions, Blockers, Action items, Open questions.\n"
            "4. For action items, include owner, due date, and status when known; otherwise say unknown.\n"
            "5. Preserve exact task ids, document paths, event ids, and dates when they appear.\n"
            "6. Keep it compact enough to paste into a shared document or message.\n\n"
            "Pitfalls:\n"
            "- Do not mix meeting notes with task execution unless the user asks for both.\n"
            "- Do not create false consensus from one participant's statement.\n"
            "- Do not include raw transcript unless explicitly requested.\n\n"
            "Verification:\n"
            "- Decisions and action items are distinguishable.\n"
            "- Unknown owners, dates, or blockers are marked instead of guessed."
        ),
        "tags": ("meeting", "notes", "minutes", "collaboration", "summary", "cccc-glue"),
    },
    "skill:cccc:readme-i18n": {
        "name": "readme-i18n",
        "description_short": (
            "Translate and localize README or documentation files while preserving Markdown structure, links, "
            "badges, anchors, code blocks, and technical terms."
        ),
        "use_when": (
            "The user asks to translate README/docs or create localized documentation variants.",
            "Markdown documentation needs multilingual adaptation without breaking links or examples.",
        ),
        "avoid_when": (
            "The task is app runtime i18n; use app-i18n-localization instead.",
            "The user wants creative copywriting rather than faithful technical localization.",
        ),
        "gotchas": (
            "Do not translate code, commands, package names, URLs, anchors, or placeholders unless explicitly intended.",
            "Preserve heading anchors when downstream links may depend on them.",
        ),
        "evidence_kind": "localized Markdown with preserved structure and technical tokens",
        "capsule_text": (
            "You are the readme-i18n skill for CCCC agents.\n\n"
            "Use this skill when translating or localizing README files and technical Markdown documentation.\n\n"
            "Procedure:\n"
            "1. Identify source language, target language, audience, and whether localization should be literal or adapted.\n"
            "2. Preserve Markdown structure, tables, links, badges, anchors, code fences, commands, env vars, paths, and placeholders.\n"
            "3. Translate explanatory prose and UI-facing text while keeping technical names stable.\n"
            "4. Keep examples executable; do not localize code or CLI flags.\n"
            "5. If creating a new localized file, match the repo's existing naming convention.\n\n"
            "Pitfalls:\n"
            "- Do not break intra-document links or generated table-of-contents anchors.\n"
            "- Do not translate proper nouns or package identifiers inconsistently.\n"
            "- Do not introduce claims absent from the source document.\n\n"
            "Verification:\n"
            "- Links, code fences, commands, and placeholders remain intact.\n"
            "- The localized text is natural in the target language and faithful to the source."
        ),
        "tags": ("readme", "i18n", "localization", "translation", "docs", "markdown", "cccc-glue"),
    },
    "skill:cccc:release-notes": {
        "name": "release-notes",
        "description_short": (
            "Turn commits, diffs, issues, or completed work into concise release notes with user impact, "
            "breaking changes, fixes, and verification."
        ),
        "use_when": (
            "The user asks for release notes, changelog entries, release summaries, or shipped-work recaps.",
            "A set of changes needs to be explained for users, operators, or developers.",
        ),
        "avoid_when": (
            "The user wants a code review or root-cause analysis instead of release communication.",
            "There is no source material about what changed.",
        ),
        "gotchas": (
            "Write from user impact, not internal implementation trivia.",
            "Call out breaking changes, migration steps, and known issues explicitly.",
        ),
        "evidence_kind": "release-note sections tied to source changes or shipped items",
        "capsule_text": (
            "You are the release-notes skill for CCCC agents.\n\n"
            "Use this skill to turn completed work, commits, diffs, issues, or task summaries into release notes.\n\n"
            "Procedure:\n"
            "1. Identify the audience: users, operators, developers, or internal stakeholders.\n"
            "2. Group changes by user-visible value: Added, Changed, Fixed, Removed, Security, Breaking changes, Known issues.\n"
            "3. Prefer concrete impact over implementation details.\n"
            "4. Include migration steps, config changes, or compatibility notes when relevant.\n"
            "5. Mention verification only when there is actual evidence.\n\n"
            "Pitfalls:\n"
            "- Do not claim a fix shipped if the source only shows a plan.\n"
            "- Do not hide breaking changes under generic changed bullets.\n"
            "- Do not include noisy commit-by-commit narration unless asked.\n\n"
            "Verification:\n"
            "- Each note maps to a source change, task, issue, or observed outcome.\n"
            "- Breaking changes and known risks are explicit."
        ),
        "tags": ("release", "changelog", "notes", "documentation", "collaboration", "cccc-glue"),
    },
    "skill:cccc:api-interface-design": {
        "name": "api-interface-design",
        "description_short": (
            "Design or review APIs, daemon ops, MCP tools, event contracts, and request/response shapes "
            "for clarity, compatibility, and low cognitive load."
        ),
        "use_when": (
            "The task changes an API, tool schema, daemon operation, event payload, or integration boundary.",
            "A user asks whether an interface is clean, stable, or easy for agents/clients to use.",
        ),
        "avoid_when": (
            "The task is purely internal implementation with no boundary or contract impact.",
            "The user only needs a quick code patch and no interface decision is involved.",
        ),
        "gotchas": (
            "Prefer contract-first changes and backward compatibility unless the user explicitly accepts a break.",
            "Avoid adding optional fields or fallback states that make the mental model harder.",
        ),
        "evidence_kind": "interface review or contract plan with compatibility notes",
        "capsule_text": (
            "You are the api-interface-design skill for CCCC agents.\n\n"
            "Use this skill when designing or reviewing daemon ops, MCP tools, API contracts, event shapes, "
            "request payloads, response fields, or cross-component boundaries.\n\n"
            "Procedure:\n"
            "1. Identify the caller, callee, state owner, and compatibility boundary.\n"
            "2. State the smallest contract that satisfies the workflow.\n"
            "3. Prefer explicit states, stable field names, and one source of truth.\n"
            "4. Check backward compatibility, failure modes, idempotency, and observability.\n"
            "5. Keep ports thin; do not move durable state into UI, CLI, MCP, or IM adapters.\n\n"
            "Pitfalls:\n"
            "- Do not add multiple equivalent fields unless there is a migration reason.\n"
            "- Do not encode policy in display copy.\n"
            "- Do not hide real failures behind success-looking compatibility fallbacks.\n\n"
            "Verification:\n"
            "- The interface has a clear owner, input contract, output contract, failure shape, and migration path."
        ),
        "tags": ("api", "interface", "contract", "mcp", "daemon", "events", "cccc-glue"),
    },
    "skill:cccc:artifact-qa": {
        "name": "artifact-qa",
        "description_short": (
            "Verify generated artifacts such as Markdown, HTML, PDFs, spreadsheets, decks, screenshots, "
            "and reports for completeness, readability, and requested format."
        ),
        "use_when": (
            "A task produces a user-visible file, document, screenshot, deck, table, or report.",
            "The user asks whether an artifact is complete, readable, or matches the requested output.",
        ),
        "avoid_when": (
            "There is no artifact to inspect or the task is only planning.",
            "Verification would require an unavailable proprietary viewer or credentialed service.",
        ),
        "gotchas": (
            "Opening or parsing the artifact is better evidence than trusting file existence.",
            "Check the user's requested format, not only whether some output was created.",
        ),
        "evidence_kind": "artifact inspection result with path, checks, and pass/fail notes",
        "capsule_text": (
            "You are the artifact-qa skill for CCCC agents.\n\n"
            "Use this skill to verify user-visible generated artifacts before reporting completion.\n\n"
            "Procedure:\n"
            "1. Identify the requested artifact type, path, audience, and acceptance criteria.\n"
            "2. Verify the artifact exists and is non-empty.\n"
            "3. Inspect it with the best available local method: parse, render, screenshot, open metadata, or sample content.\n"
            "4. Check format, structure, readability, missing sections, broken links, placeholder text, and obvious corruption.\n"
            "5. Report exact evidence and fix issues before claiming completion when possible.\n\n"
            "Pitfalls:\n"
            "- Do not treat a saved file path as proof the artifact is usable.\n"
            "- Do not verify a different format than the user requested.\n"
            "- Do not expose private content unnecessarily when summarizing checks.\n\n"
            "Verification:\n"
            "- The final report names the artifact path and the concrete checks performed."
        ),
        "tags": ("artifact", "qa", "verification", "documents", "reports", "files", "cccc-glue"),
    },
    "skill:cccc:browser-qa": {
        "name": "browser-qa",
        "description_short": (
            "Plan and perform browser-facing QA for local web apps: load state, layout, interaction, console errors, "
            "responsive behavior, and visual regressions."
        ),
        "use_when": (
            "The task involves verifying a web UI, frontend interaction, responsive layout, or rendered page.",
            "A user reports a browser/UI regression or asks for visual QA.",
        ),
        "avoid_when": (
            "No browser or rendered artifact is available and the user only wants static code review.",
            "The task would require broad browser automation permissions not available in the runtime.",
        ),
        "gotchas": (
            "Use available local browser or Playwright tooling when present; do not assume an MCP exists.",
            "Check the actual failing viewport or trigger path, not just a happy-path load.",
        ),
        "evidence_kind": "browser QA checklist with URL, viewport, interaction, and observed result",
        "capsule_text": (
            "You are the browser-qa skill for CCCC agents.\n\n"
            "Use this skill when a web UI needs rendered verification.\n\n"
            "Procedure:\n"
            "1. Identify the target URL, route, viewport, state, and trigger path.\n"
            "2. Start or confirm the local dev server only when needed.\n"
            "3. Inspect real rendered behavior with available browser tooling, screenshots, or Playwright.\n"
            "4. Check loading, layout, overflow, interaction, console errors, network failures, and mobile/desktop breakpoints.\n"
            "5. Re-run the same trigger path after fixes and record observed evidence.\n\n"
            "Pitfalls:\n"
            "- Do not infer UI correctness from code alone when rendered verification is practical.\n"
            "- Do not apply landing-page criteria to operational product surfaces.\n"
            "- Do not leave a dev server running if the task does not need it anymore.\n\n"
            "Verification:\n"
            "- The result names the URL, viewport, interaction path, and observed pass/fail evidence."
        ),
        "tags": ("browser", "qa", "frontend", "playwright", "visual", "responsive", "cccc-glue"),
    },
    "skill:cccc:code-simplification": {
        "name": "code-simplification",
        "description_short": (
            "Simplify code paths, remove redundant mechanisms, and reduce states while preserving behavior "
            "and tests."
        ),
        "use_when": (
            "The user asks to simplify, de-risk, reduce noise, remove fallback branches, or clean a mechanism.",
            "A code path has duplicate states, competing sources of truth, or excessive defensive layers.",
        ),
        "avoid_when": (
            "The task requires a feature addition and simplification would be speculative.",
            "Behavior is not understood well enough to safely remove paths.",
        ),
        "gotchas": (
            "Preserve user-visible contracts and migration paths.",
            "Remove one mechanism at a time and verify the original trigger path.",
        ),
        "evidence_kind": "simplification patch summary with removed states and regression evidence",
        "capsule_text": (
            "You are the code-simplification skill for CCCC agents.\n\n"
            "Use this skill when a mechanism should become simpler, clearer, or less noisy.\n\n"
            "Procedure:\n"
            "1. Identify the behavior that must remain true and the evidence that proves it.\n"
            "2. Locate duplicate states, fallback branches, parallel mechanisms, and unclear ownership.\n"
            "3. Prefer removing or merging mechanisms over adding new guards.\n"
            "4. Make the smallest reversible change that reduces cognitive load.\n"
            "5. Run the original failing or risky path plus focused regression tests.\n\n"
            "Pitfalls:\n"
            "- Do not delete compatibility paths unless migration impact is understood.\n"
            "- Do not hide complexity behind a new abstraction unless it removes real duplication.\n"
            "- Do not claim simplification if the number of states or special cases increased.\n\n"
            "Verification:\n"
            "- The summary names what was removed or merged and which behavior still passes."
        ),
        "tags": ("code", "simplification", "refactor", "cleanup", "maintainability", "cccc-glue"),
    },
    "skill:cccc:copy-editing": {
        "name": "copy-editing",
        "description_short": (
            "Edit product copy, documentation prose, prompts, and user-facing text for clarity, tone, brevity, "
            "and actionability without changing facts."
        ),
        "use_when": (
            "The user asks to polish, rewrite, tighten, or improve wording.",
            "A product surface, doc, prompt, or message needs clearer user-facing copy.",
        ),
        "avoid_when": (
            "The user wants the underlying task executed instead of wording improvement.",
            "The source facts are ambiguous and require research before editing.",
        ),
        "gotchas": (
            "Preserve intent, constraints, facts, and required terminology.",
            "Avoid making copy sound more confident than the evidence supports.",
        ),
        "evidence_kind": "edited copy or concise copy review with rationale",
        "capsule_text": (
            "You are the copy-editing skill for CCCC agents.\n\n"
            "Use this skill to improve wording without changing meaning.\n\n"
            "Procedure:\n"
            "1. Identify audience, channel, tone, and required facts.\n"
            "2. Preserve meaning, constraints, terminology, dates, names, and numbers.\n"
            "3. Improve clarity, structure, brevity, and actionability.\n"
            "4. Remove filler, meta commentary, and ambiguous pronouns.\n"
            "5. Return the edited copy directly when the user asks for an artifact.\n\n"
            "Pitfalls:\n"
            "- Do not invent benefits, guarantees, or evidence.\n"
            "- Do not over-polish operational text into marketing language.\n"
            "- Do not strip useful technical specificity.\n\n"
            "Verification:\n"
            "- The output is clearer and still faithful to the source."
        ),
        "tags": ("copy", "editing", "writing", "docs", "prompt", "product", "cccc-glue"),
    },
    "skill:cccc:documentation-adr": {
        "name": "documentation-adr",
        "description_short": (
            "Create or review technical documentation and architecture decision records with context, tradeoffs, "
            "decision, consequences, and rollback notes."
        ),
        "use_when": (
            "The user asks for technical docs, an ADR, design note, architecture rationale, or decision write-up.",
            "A code or product decision needs durable explanation for future maintainers.",
        ),
        "avoid_when": (
            "The user only wants a quick answer or implementation patch.",
            "No actual decision or architecture context is available.",
        ),
        "gotchas": (
            "Separate facts, decisions, options, tradeoffs, and consequences.",
            "Do not turn uncertain ideas into accepted architecture.",
        ),
        "evidence_kind": "technical doc or ADR with decision, tradeoffs, and consequences",
        "capsule_text": (
            "You are the documentation-adr skill for CCCC agents.\n\n"
            "Use this skill for technical documentation, ADRs, and durable architecture notes.\n\n"
            "Procedure:\n"
            "1. Identify the audience, decision status, scope, and source evidence.\n"
            "2. Structure ADRs as Context, Decision, Options, Rationale, Consequences, Risks, Rollback.\n"
            "3. For docs, lead with what the reader needs to do or understand.\n"
            "4. Preserve exact commands, paths, APIs, dates, and compatibility constraints.\n"
            "5. Keep speculative future work separate from current behavior.\n\n"
            "Pitfalls:\n"
            "- Do not document planned behavior as shipped behavior.\n"
            "- Do not omit tradeoffs or rollback costs.\n"
            "- Do not duplicate source comments when a concise reference is enough.\n\n"
            "Verification:\n"
            "- A future maintainer can understand what changed, why, and what breaks if it is reversed."
        ),
        "tags": ("documentation", "adr", "architecture", "design", "technical-writing", "cccc-glue"),
    },
    "skill:cccc:report-writing": {
        "name": "report-writing",
        "description_short": (
            "Write evidence-based reports, audits, reviews, and investigation summaries with clear findings, "
            "scope, proof, and next steps."
        ),
        "use_when": (
            "The user asks for a report, audit, investigation summary, review memo, or written findings.",
            "A completed analysis needs to become a durable artifact.",
        ),
        "avoid_when": (
            "A short chat answer is enough.",
            "The evidence has not been collected and the user asked for investigation first.",
        ),
        "gotchas": (
            "Lead with findings and evidence; do not bury risks in narrative.",
            "Separate observed facts from interpretation and recommendations.",
        ),
        "evidence_kind": "durable report with scope, findings, evidence, risks, and next steps",
        "capsule_text": (
            "You are the report-writing skill for CCCC agents.\n\n"
            "Use this skill to turn completed analysis into a durable report or audit artifact.\n\n"
            "Procedure:\n"
            "1. Define scope, audience, sources, and decision need.\n"
            "2. Lead with key findings ordered by impact.\n"
            "3. Attach evidence: commands, files, links, event ids, screenshots, data, or test results.\n"
            "4. Separate facts, analysis, risks, recommendations, and unresolved questions.\n"
            "5. Keep the report actionable and avoid process narration.\n\n"
            "Pitfalls:\n"
            "- Do not write a polished report before evidence exists.\n"
            "- Do not dilute severe findings with generic summaries.\n"
            "- Do not omit test gaps or assumptions.\n\n"
            "Verification:\n"
            "- Every major conclusion is traceable to evidence or explicitly marked as an inference."
        ),
        "tags": ("report", "audit", "review", "investigation", "writing", "evidence", "cccc-glue"),
    },
    "skill:cccc:slide-deck-outline": {
        "name": "slide-deck-outline",
        "description_short": (
            "Plan concise slide decks and presentation narratives with audience, storyline, slide structure, "
            "speaker notes, and artifact QA expectations."
        ),
        "use_when": (
            "The user asks for PPT, slides, deck, presentation, pitch, or briefing slides.",
            "A report or idea needs to become a presentation outline before artifact generation.",
        ),
        "avoid_when": (
            "The user requires a final editable PPTX and no generation tool is available.",
            "A written report is more appropriate than slides.",
        ),
        "gotchas": (
            "Clarify whether the requested deliverable is an outline, Markdown slides, HTML slides, or editable PPTX.",
            "Keep one primary message per slide.",
        ),
        "evidence_kind": "deck outline with audience, slide titles, key points, and verification needs",
        "capsule_text": (
            "You are the slide-deck-outline skill for CCCC agents.\n\n"
            "Use this skill when a task needs presentation structure or slide planning.\n\n"
            "Procedure:\n"
            "1. Identify audience, purpose, time limit, tone, and required format.\n"
            "2. Define the storyline before writing individual slides.\n"
            "3. Use one main message per slide with supporting bullets, evidence, and optional speaker notes.\n"
            "4. Mark charts, tables, screenshots, or diagrams that require separate artifact work.\n"
            "5. If a final file is requested, state the required generation path and QA checks.\n\n"
            "Pitfalls:\n"
            "- Do not pretend an outline is an editable PPTX.\n"
            "- Do not overload slides with report-length text.\n"
            "- Do not skip audience and decision goal.\n\n"
            "Verification:\n"
            "- The deck has a coherent opening, progression, conclusion, and requested artifact format."
        ),
        "tags": ("slides", "ppt", "pptx", "deck", "presentation", "briefing", "cccc-glue"),
    },
    "skill:cccc:test-driven-development": {
        "name": "test-driven-development",
        "description_short": (
            "Plan and implement changes with focused tests, regression coverage, failing-path reproduction, "
            "and clear verification evidence."
        ),
        "use_when": (
            "A code change has behavioral risk, regression risk, or a failing path that should be captured.",
            "The user asks for tests, TDD, coverage, or proof that a fix works.",
        ),
        "avoid_when": (
            "The change is documentation-only or too trivial to justify test churn.",
            "The test harness is unavailable and the user only asked for analysis.",
        ),
        "gotchas": (
            "Test the user-visible contract, not just implementation details.",
            "Keep tests focused; do not broaden scope to unrelated behavior.",
        ),
        "evidence_kind": "test plan, test patch, and observed test result",
        "capsule_text": (
            "You are the test-driven-development skill for CCCC agents.\n\n"
            "Use this skill when a behavior change should be protected by tests.\n\n"
            "Procedure:\n"
            "1. Identify the behavior contract, regression risk, and smallest failing path.\n"
            "2. Add or update the narrowest meaningful test before or alongside the fix.\n"
            "3. Prefer black-box behavior assertions over brittle internals unless the contract is internal.\n"
            "4. Run the focused test first, then relevant surrounding tests.\n"
            "5. Report exact commands and observed results.\n\n"
            "Pitfalls:\n"
            "- Do not add snapshot churn or broad fixtures for a narrow bug.\n"
            "- Do not claim coverage from a test that does not fail without the fix.\n"
            "- Do not skip manual verification when UI or artifact behavior needs it.\n\n"
            "Verification:\n"
            "- The changed behavior has a focused test or a clear reason why test coverage was not practical."
        ),
        "tags": ("testing", "tdd", "regression", "pytest", "verification", "code", "cccc-glue"),
    },
    "skill:cccc:frontend-ui-engineering": {
        "name": "frontend-ui-engineering",
        "description_short": (
            "Build frontend product surfaces with existing design systems, responsive structure, state clarity, "
            "interaction ergonomics, and rendered verification."
        ),
        "use_when": (
            "The user asks to build or change a frontend UI, app screen, dashboard, or product workflow.",
            "A UI implementation needs both engineering correctness and product-quality behavior.",
        ),
        "avoid_when": (
            "The task is a visual review only; use taste-review or browser-qa instead.",
            "The task is backend-only or no user-facing surface is involved.",
        ),
        "gotchas": (
            "Respect the existing design system and product surface type before adding new visual language.",
            "Rendered behavior and responsive layout matter as much as component code.",
        ),
        "evidence_kind": "frontend patch plus rendered or test evidence for the affected workflow",
        "capsule_text": (
            "You are the frontend-ui-engineering skill for CCCC agents.\n\n"
            "Use this skill when implementing or changing product UI.\n\n"
            "Procedure:\n"
            "1. Classify the surface: operational product, dashboard, form/workflow, landing/showcase, or game.\n"
            "2. Identify the existing component conventions, tokens, spacing, typography, state model, and data flow.\n"
            "3. Build the actual workflow, not a marketing placeholder or decorative shell.\n"
            "4. Keep controls predictable: icons for tools, inputs for values, menus for choices, toggles for booleans.\n"
            "5. Verify responsive layout, overflow, empty/loading/error states, and primary interactions.\n\n"
            "Pitfalls:\n"
            "- Do not add a landing hero when the user asked for an app/tool/workspace.\n"
            "- Do not create card mosaics or decorative gradients for routine product surfaces.\n"
            "- Do not rely on code inspection when a browser check is practical.\n\n"
            "Verification:\n"
            "- The affected route or component renders correctly at relevant desktop/mobile breakpoints."
        ),
        "tags": ("frontend", "ui", "ux", "product", "react", "browser", "cccc-glue"),
    },
    "skill:cccc:git-workflow-versioning": {
        "name": "git-workflow-versioning",
        "description_short": (
            "Handle git hygiene, diff review, branch-safe commits, changelog boundaries, and versioning "
            "without disturbing unrelated user changes."
        ),
        "use_when": (
            "The user asks to commit, inspect diffs, pull, merge, review remote changes, or prepare release/version notes.",
            "A repo operation must preserve dirty worktree changes and produce clear evidence.",
        ),
        "avoid_when": (
            "No repository operation is involved.",
            "The user has not authorized a commit, destructive operation, pull, or merge.",
        ),
        "gotchas": (
            "Never revert unrelated user changes.",
            "Inspect dirty state before mutating git history or committing.",
        ),
        "evidence_kind": "git status/diff summary, action taken, commit hash or explicit no-commit state",
        "capsule_text": (
            "You are the git-workflow-versioning skill for CCCC agents.\n\n"
            "Use this skill for git operations, commits, pulls, merges, release boundaries, and diff hygiene.\n\n"
            "Procedure:\n"
            "1. Read `git status --short` before any mutation.\n"
            "2. Separate your changes from unrelated user changes and preserve both.\n"
            "3. For remote changes, inspect before pulling when the user asks for review.\n"
            "4. Commit only when explicitly requested, with a focused message and only in-scope files.\n"
            "5. After mutation, report status, commit hash if any, and verification commands.\n\n"
            "Pitfalls:\n"
            "- Do not use destructive checkout/reset unless explicitly requested.\n"
            "- Do not include ignored runtime state or generated noise.\n"
            "- Do not claim clean state without reading it.\n\n"
            "Verification:\n"
            "- Final response includes the repo state and exact git action performed or deferred."
        ),
        "tags": ("git", "versioning", "commit", "merge", "release", "diff", "cccc-glue"),
    },
    "skill:cccc:performance-optimization": {
        "name": "performance-optimization",
        "description_short": (
            "Improve runtime, UI, query, or test performance with baseline measurement, bottleneck evidence, "
            "bounded changes, and regression checks."
        ),
        "use_when": (
            "The user reports slow behavior, heavy tests, UI lag, high latency, or wants optimization.",
            "A proposed optimization needs a baseline and proof of impact.",
        ),
        "avoid_when": (
            "There is no measurable bottleneck and the user did not ask for performance work.",
            "The optimization would add complexity without clear observed payoff.",
        ),
        "gotchas": (
            "Measure before optimizing whenever practical.",
            "Prefer removing work or narrowing scope before adding caches or parallelism.",
        ),
        "evidence_kind": "baseline, bottleneck hypothesis, change, and before/after verification",
        "capsule_text": (
            "You are the performance-optimization skill for CCCC agents.\n\n"
            "Use this skill when performance is the actual task.\n\n"
            "Procedure:\n"
            "1. Identify the user-visible metric: latency, throughput, memory, render time, test time, or payload size.\n"
            "2. Capture a baseline or concrete symptom before changing code.\n"
            "3. Form one bottleneck hypothesis and make the smallest reversible change.\n"
            "4. Prefer deleting redundant work, batching, indexing, or narrowing scope before adding caches.\n"
            "5. Compare before/after and run focused correctness checks.\n\n"
            "Pitfalls:\n"
            "- Do not optimize by intuition alone when measurement is feasible.\n"
            "- Do not add stale caches without invalidation evidence.\n"
            "- Do not trade correctness or clarity for unproven speed.\n\n"
            "Verification:\n"
            "- The final report states baseline, observed improvement or no improvement, and residual risk."
        ),
        "tags": ("performance", "optimization", "latency", "profiling", "tests", "frontend", "cccc-glue"),
    },
    "skill:cccc:product-requirements": {
        "name": "product-requirements",
        "description_short": (
            "Turn product ideas, user feedback, or feature asks into practical requirements, success criteria, "
            "scope boundaries, and rollout risks."
        ),
        "use_when": (
            "The user asks for product planning, PRD-like requirements, scope definition, or feature tradeoffs.",
            "A broad idea needs to become implementable without overbuilding.",
        ),
        "avoid_when": (
            "The user already gave exact implementation instructions and wants code changes now.",
            "The ask is a simple copy edit or meeting summary.",
        ),
        "gotchas": (
            "Start with user outcome and workflow, not feature inventory.",
            "Keep MVP scope small and name explicit non-goals.",
        ),
        "evidence_kind": "requirements brief with user outcome, scope, success criteria, and non-goals",
        "capsule_text": (
            "You are the product-requirements skill for CCCC agents.\n\n"
            "Use this skill to shape product ideas into actionable requirements.\n\n"
            "Procedure:\n"
            "1. Identify target user, problem, current workflow, desired outcome, and constraints.\n"
            "2. Define MVP scope, non-goals, success criteria, and failure modes.\n"
            "3. Map the core user flow and the smallest implementation slice that proves value.\n"
            "4. Call out risk, dependencies, analytics/evidence needs, and rollback path.\n"
            "5. Keep requirements concrete enough for engineering without pretending unknowns are decided.\n\n"
            "Pitfalls:\n"
            "- Do not turn product planning into a feature wishlist.\n"
            "- Do not add governance or settings unless there is immediate payoff.\n"
            "- Do not bury hard tradeoffs in vague roadmap language.\n\n"
            "Verification:\n"
            "- A developer can tell what to build first, what not to build, and how success is judged."
        ),
        "tags": ("product", "requirements", "prd", "planning", "scope", "mvp", "cccc-glue"),
    },
    "skill:cccc:security-privacy-review": {
        "name": "security-privacy-review",
        "description_short": (
            "Review implementation, configuration, or product changes for practical security, privacy, secret, "
            "permission, and data-exposure risks."
        ),
        "use_when": (
            "A change touches auth, permissions, secrets, network calls, user data, files, browser automation, or external services.",
            "The user asks for security/privacy risk review.",
        ),
        "avoid_when": (
            "The task asks for offensive exploitation or unauthorized access.",
            "The review would require secrets or production data that are not available.",
        ),
        "gotchas": (
            "Focus on practical risk and concrete mitigations, not generic security checklists.",
            "Separate confirmed vulnerabilities from hardening suggestions.",
        ),
        "evidence_kind": "risk review with severity, evidence, impact, mitigation, and residual risk",
        "capsule_text": (
            "You are the security-privacy-review skill for CCCC agents.\n\n"
            "Use this skill for defensive review of security, privacy, permissions, and data exposure.\n\n"
            "Procedure:\n"
            "1. Identify protected assets: secrets, user data, local files, credentials, tokens, permissions, and external calls.\n"
            "2. Trace trust boundaries and who can trigger the behavior.\n"
            "3. Look for concrete issues: secret leakage, broad permissions, unsafe defaults, injection, path traversal, auth bypass, privacy exposure.\n"
            "4. Rank findings by realistic impact and exploitability.\n"
            "5. Recommend minimal mitigations and note residual risk.\n\n"
            "Pitfalls:\n"
            "- Do not provide offensive steps beyond what is needed to explain defensive risk.\n"
            "- Do not label theoretical issues as confirmed bugs without evidence.\n"
            "- Do not ignore UX/product tradeoffs of stricter controls.\n\n"
            "Verification:\n"
            "- Every finding has evidence, impact, and a mitigation or explicit accepted risk."
        ),
        "tags": ("security", "privacy", "review", "secrets", "permissions", "risk", "cccc-glue"),
    },
    "skill:cccc:spec-driven-development": {
        "name": "spec-driven-development",
        "description_short": (
            "Use a lightweight spec before implementation when behavior, contracts, user flows, or acceptance "
            "criteria need alignment."
        ),
        "use_when": (
            "The user asks for a spec, design plan, acceptance criteria, or implementation plan before coding.",
            "A change spans multiple modules or unclear behavior contracts.",
        ),
        "avoid_when": (
            "The task is a narrow obvious patch and the user asked to implement directly.",
            "Spec writing would duplicate an already clear approved scope.",
        ),
        "gotchas": (
            "Specs should reduce ambiguity, not become ceremony.",
            "Do not replace CCCC task state or user-approved scope with a parallel process.",
        ),
        "evidence_kind": "lightweight spec with behavior, scope, acceptance criteria, and verification plan",
        "capsule_text": (
            "You are the spec-driven-development skill for CCCC agents.\n\n"
            "Use this skill when a lightweight spec will reduce implementation risk.\n\n"
            "Procedure:\n"
            "1. State the user goal, current behavior, target behavior, and non-goals.\n"
            "2. Identify affected contracts, files/modules, data flow, and compatibility risks.\n"
            "3. Define acceptance criteria and verification steps before coding.\n"
            "4. Keep the spec short enough to guide implementation in the current turn.\n"
            "5. After approval or clear action intent, implement against the spec and update it only if facts change.\n\n"
            "Pitfalls:\n"
            "- Do not create a heavyweight planning artifact for a one-line fix.\n"
            "- Do not let spec wording override newer user instructions.\n"
            "- Do not mark acceptance criteria met without evidence.\n\n"
            "Verification:\n"
            "- The implementation can be checked directly against the acceptance criteria."
        ),
        "tags": ("spec", "planning", "acceptance", "implementation", "contracts", "cccc-glue"),
    },
    "skill:cccc:runtime-bootstrap": {
        "name": "runtime-bootstrap",
        "description_short": (
            "Diagnose CCCC daemon/web startup, actor runtime launch, MCP injection, "
            "bind/LAN reachability, and shutdown residue issues."
        ),
        "use_when": (
            "CCCC daemon or Web fails to start, bind, or stay reachable.",
            "Actor runtime launch, MCP injection, or shutdown cleanup looks broken.",
        ),
        "avoid_when": (
            "The task is normal product work, not runtime diagnosis.",
        ),
        "gotchas": (
            "Separate configured Web binding from the live listener before changing settings or restarting anything.",
            "Treat process residue and stale pid files as evidence to verify, not proof that the current runtime is healthy.",
        ),
        "evidence_kind": "debug snapshot plus terminal/log proof for the failing layer",
        "capsule_text": (
            "You are the runtime-bootstrap skill for CCCC runtime diagnosis.\n\n"
            "Use this skill when the task is about daemon or web startup failure, port bind or LAN "
            "reachability, actor launch/runtime state, MCP injection, or residue left after shutdown.\n\n"
            "Protocol:\n"
            "1. Restate the exact symptom and isolate the failing layer before changing anything.\n"
            "2. Gather evidence first; prefer read-only inspection and existing diagnostics/runtime tools.\n"
            "3. Check one layer at a time: process start -> bind/port -> group/actor runtime -> MCP "
            "injection -> shutdown cleanup.\n"
            "4. Report findings as: Symptom, Evidence, Failed layer, Most likely root cause, Next safe action.\n"
            "5. Do not kill, restart, or mutate runtime state unless the user explicitly asks after evidence is gathered.\n"
            "6. Prefer the smallest reversible fix. If two hypotheses fail, stop stacking guards and surface evidence."
        ),
        "tags": ("runtime", "bootstrap", "diagnostics", "daemon", "web", "mcp"),
        "requires_capabilities": ("pack:diagnostics", "pack:group-runtime"),
    },
    "skill:cccc:standup-summary": {
        "name": "standup-summary",
        "description_short": (
            "Produce concise standup summaries with yesterday/current work, blockers, today's plan, "
            "standby status, and follow-up needs."
        ),
        "use_when": (
            "The user asks for a standup, daily update, morning meeting, status rollup, or team check-in.",
            "Multiple agents report work/blockers and the result needs a compact coordination summary.",
        ),
        "avoid_when": (
            "The task is a full meeting minutes request; use meeting-notes instead.",
            "The user asks to execute a task rather than summarize status.",
        ),
        "gotchas": (
            "Standby is a valid status; do not turn no-update into fake work.",
            "Preserve blockers and stale task ids exactly.",
        ),
        "evidence_kind": "standup summary with work, blockers, plan, and follow-ups",
        "capsule_text": (
            "You are the standup-summary skill for CCCC daily coordination.\n\n"
            "Use this skill when the user asks for a standup, morning meeting, daily update, or status rollup.\n\n"
            "Procedure:\n"
            "1. Collect each participant's current work, blockers, today's plan, and standby/no-update status from available context.\n"
            "2. Preserve task ids, actor names, dates, blockers, and explicit commitments.\n"
            "3. Summarize by participant first when coordination matters; summarize by blocker first when risk matters.\n"
            "4. Keep output short: Current work, Blockers, Today's plan, Follow-ups.\n"
            "5. Mark missing reports as missing; do not infer status from silence.\n\n"
            "Pitfalls:\n"
            "- Do not expand a standup into a project report unless asked.\n"
            "- Do not treat stale lifecycle/test cards as newly changed without evidence.\n"
            "- Do not invent blockers or progress.\n\n"
            "Verification:\n"
            "- Every participant with available input is represented.\n"
            "- Blockers and follow-ups are explicit and actionable."
        ),
        "tags": ("standup", "daily", "status", "coordination", "collaboration", "cccc-glue"),
    },
}


def all_builtin_pack_ids() -> List[str]:
    return sorted(BUILTIN_CAPABILITY_PACKS.keys())


def all_builtin_skill_ids() -> List[str]:
    return sorted(BUILTIN_CAPSULE_SKILLS.keys())


def core_tool_name_set() -> Set[str]:
    return set(CORE_TOOL_NAMES)


def all_pack_tool_name_set() -> Set[str]:
    names: Set[str] = set()
    for pack in BUILTIN_CAPABILITY_PACKS.values():
        for tool_name in pack.get("tool_names", ()):  # type: ignore[arg-type]
            names.add(str(tool_name))
    return names


def web_model_advertised_tool_names(all_tool_names: Iterable[str]) -> Set[str]:
    """Return the stable built-in tool schema advertised to Web Model clients."""
    excluded = set(WEB_MODEL_ADVERTISED_EXCLUDED_TOOLS)
    return {str(name).strip() for name in all_tool_names if str(name).strip() and str(name).strip() not in excluded}


def resolve_core_tool_names(
    *,
    actor_role: str = "",
    is_pet: bool = False,
    is_voice_secretary: bool = False,
    is_web_model: bool = False,
) -> Set[str]:
    if bool(is_voice_secretary):
        return set(VOICE_SECRETARY_CORE_TOOLS)
    if bool(is_pet):
        return set(PET_CORE_TOOLS)
    if bool(is_web_model):
        return set(WEB_MODEL_CORE_TOOLS)
    role = str(actor_role or "").strip().lower()
    if role == "peer":
        return set(CORE_BASIC_TOOLS)
    return set(CORE_TOOL_NAMES)


def resolve_visible_tool_names(
    enabled_capability_ids: Iterable[str],
    *,
    actor_role: str = "",
    is_pet: bool = False,
    is_voice_secretary: bool = False,
    is_web_model: bool = False,
) -> Set[str]:
    visible = resolve_core_tool_names(
        actor_role=actor_role,
        is_pet=is_pet,
        is_voice_secretary=is_voice_secretary,
        is_web_model=is_web_model,
    )
    for cap_id in enabled_capability_ids:
        cap = BUILTIN_CAPABILITY_PACKS.get(str(cap_id))
        if not isinstance(cap, dict):
            continue
        for tool_name in cap.get("tool_names", ()):  # type: ignore[arg-type]
            visible.add(str(tool_name))
    return visible
