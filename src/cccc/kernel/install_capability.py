"""Built-in CCCC install capability definition."""

from __future__ import annotations

INSTALL_CAPABILITY_ID = "skill:cccc:install"

INSTALL_CAPABILITY_RECORD = {
    "name": "install",
    "description_short": (
        "Install skills, MCP toolpacks, CCCC capabilities, and GitHub repositories through the "
        "CCCC capability registry, then enable them for the group slash surface."
    ),
    "use_when": (
        "The user types /install followed by a skill name, capability id, repo slug, URL, or local path.",
        "A requested capability needs to be imported into the CCCC capability registry and enabled for the group slash surface.",
        "An installed skill should appear in the group's slash command menu after installation.",
    ),
    "avoid_when": (
        "The user is asking how installation works but does not want the environment changed.",
        "The candidate requires secrets, broad local control, or untrusted code execution and has not been vetted.",
    ),
    "gotchas": (
        "Do not assume every install target is GitHub; classify the target before choosing a workflow.",
        "Default /install behavior is CCCC capability installation, not Codex local skill-package installation.",
    ),
    "evidence_kind": "installed/enabled target, source, destination or capability_id, and verification result",
    "capsule_text": (
        "You are the install skill for CCCC agents.\n\n"
        "Default /install behavior is CCCC capability installation, not Codex local skill-package installation.\n\n"
        "Default action: call cccc_capability_install for the target with scope=group. "
        "The install operation resolves the target into CCCC capability records, imports them into the registry, "
        "enables group scope, and returns use-ready capability ids. "
        "Activate, assign, autoload, or use only after the CCCC capability record exists.\n\n"
        "Use this skill when a chat message invokes /install or asks to install a capability, skill, "
        "MCP toolpack, or GitHub repository.\n\n"
        "Procedure:\n"
        "1. Classify the target: curated skill name, GitHub URL, owner/repo slug, generic URL, local path, "
        "skill:/mcp:/pack: capability id, or free-form install hint.\n"
        "2. Prefer cccc_capability_install for /install targets. Fall back to capability search/import only "
        "when the target is already a concrete capability record or the install operation reports unsupported.\n"
        "3. Vet third-party or executable sources before enabling them; inspect manifests, SKILL.md, scripts, hooks, "
        "MCP configs, permissions, and required secrets.\n"
        "4. For GitHub repositories, remote SKILL.md URLs, and local SKILL.md files/directories, install the minimal matching CCCC capability records through cccc_capability_install. "
        "Do not bypass the registry by installing into Codex's local skills directory.\n"
        "5. For GitHub skill repositories, pass the repository URL or owner/repo as cccc_capability_install.target. If the repo "
        "contains multiple skills/*/SKILL.md files, the daemon must import and enable each SKILL.md as its own CCCC "
        "skill capability; do not create one aggregate repository skill.\n"
        "6. Any additional activate, assign, autoload, or use step must operate against the imported CCCC capability_id.\n"
        "7. Install and enable only the requested in-scope item. Do not broadly import an entire repository unless the "
        "source layout requires it.\n"
        "8. Verify the result with capability_state and capability search/overview. "
        "active_capsule_skills, enabled_capabilities, dynamic_tools, or external_binding_states should show the enabled item.\n"
        "9. Report the capability_id, source, group scope, and verification evidence.\n\n"
        "Pitfalls:\n"
        "- Do not treat CCCC /install as a full local Codex skill package install.\n"
        "- Do not install into Codex's local skills directory or use the Codex skill-installer workflow for /install.\n"
        "- Generic URLs and local paths must go through cccc_capability_install and become CCCC capability records; report unsupported only when the install operation rejects the target.\n"
        "- Do not collapse a multi-skill GitHub repository into one capability; slash commands are driven by the individual active_capsule_skills rows.\n"
        "- Do not bypass capability policy, block state, or required secret checks.\n"
        "- Do not install untrusted executable code without a minimal safety review.\n\n"
        "Verification:\n"
        "- The final response names exactly what CCCC capability was installed, enabled for group scope, and how it was verified.\n"
        "- If blocked, the response names the blocker and owner instead of pretending installation succeeded."
    ),
    "tags": ("install", "skill", "mcp", "capability", "github", "repository", "cccc-glue"),
    "requires_capabilities": ("pack:diagnostics",),
}
