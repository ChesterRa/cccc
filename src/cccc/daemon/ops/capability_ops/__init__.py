"""Capability registry and progressive MCP disclosure operation handlers.

This package re-exports every public symbol from its domain-specific
sub-modules so that external callers can continue to use
``from cccc.daemon.ops.capability_ops import ...`` unchanged.
"""

from __future__ import annotations

from typing import Any, Dict, Optional
from urllib.request import urlopen  # noqa: F401 – re-exported for mock compatibility

from ....contracts.v1 import DaemonResponse

from ._common import (  # noqa: F401
    _SOURCE_IDS,
    _MCP_REGISTRY_BASE,
    _MCP_REGISTRY_PAGE_LIMIT,
    _GITHUB_API_BASE,
    _RAW_GITHUB_BASE,
    _OPENCLAW_SKILLS_TREE_API,
    _OPENCLAW_SKILLS_BLOB_BASE,
    _CLAWSKILLS_DATA_URL_DEFAULT,
    _SKILL_NAME_RE,
    _ARG_TEMPLATE_RE,
    _ENV_FORWARD_TEMPLATE_RE,
    _CLAWSKILLS_ENTRY_RE,
    _STATE_LOCK,
    _CATALOG_LOCK,
    _RUNTIME_LOCK,
    _AUDIT_LOCK,
    _POLICY_LOCK,
    _REMOTE_SOURCE_CACHE_LOCK,
    _OPENCLAW_TREE_CACHE,
    _LEVEL_INDEXED,
    _LEVEL_MOUNTED,
    _LEVEL_ENABLED,
    _LEVEL_PINNED,
    _LEVELS,
    _POLICY_CACHE,
    _QUAL_QUALIFIED,
    _QUAL_BLOCKED,
    _QUAL_UNAVAILABLE,
    _QUAL_STATES,
    _error,
    _capability_root,
    _state_path,
    _catalog_path,
    _runtime_path,
    _audit_path,
    _ensure_group,
    _is_foreman,
    _normalize_scope,
    _http_get_json,
    _http_get_json_obj,
    _http_get_text,
    _env_int,
    _env_bool,
    _quota_limit,
)
from ._documents import (  # noqa: F401
    _source_state_template,
    _new_state_doc,
    _new_catalog_doc,
    _new_runtime_doc,
    _normalize_state_doc,
    _normalize_catalog_doc,
    _normalize_runtime_doc,
    _load_state_doc,
    _save_state_doc,
    _load_catalog_doc,
    _save_catalog_doc,
    _load_runtime_doc,
    _save_runtime_doc,
)
from ._runtime import (  # noqa: F401
    _runtime_artifacts,
    _runtime_capability_artifacts,
    _runtime_actor_bindings,
    _runtime_recent_success,
    _record_runtime_recent_success,
    _set_runtime_capability_artifact,
    _runtime_install_for_capability,
    _remove_runtime_capability_artifact,
    _remove_runtime_artifact_if_unreferenced,
    _set_runtime_actor_binding,
    _remove_runtime_actor_binding,
    _remove_runtime_group_capability_bindings,
    _remove_runtime_capability_bindings_all_groups,
    _append_audit_event,
)
from ._state import (  # noqa: F401
    _binding_state_allows_external_tool,
    _install_state_allows_external_tool,
    _collect_enabled_capabilities,
    _set_enabled_capability,
    _remove_capability_bindings,
    _remove_capability_bindings_all_groups,
    _set_blocked_capability,
    _unset_blocked_capability,
    _collect_blocked_capabilities,
    _has_any_binding_for_capability,
    handle_capability_enable,
    handle_capability_block,
)

from ._policy import (  # noqa: F401
    _normalize_policy_level,
    _policy_level_visible,
    _policy_default_compiled,
    _allowlist_default_source_label,
    _allowlist_user_overlay_path,
    _safe_load_yaml_mapping,
    _load_allowlist_default_doc,
    _load_allowlist_overlay_doc,
    _merge_allowlist_docs,
    _allowlist_effective_snapshot,
    _external_capability_safety_mode_from_effective_doc,
    _external_capability_safety_mode_from_policy,
    _write_allowlist_overlay_doc,
    _clear_policy_cache,
    _compile_allowlist_policy,
    _allowlist_policy,
    _allowlist_validate_overlay_doc,
    handle_capability_allowlist_get,
    handle_capability_allowlist_validate,
    handle_capability_allowlist_update,
    handle_capability_allowlist_reset,
)
from ._search import (  # noqa: F401
    _resolve_actor_role,
    _effective_policy_level,
    _display_name_from_capability_id,
    _build_builtin_search_records,
    _search_matches,
    _score_item_tokens,
    _canonicalize_actor_hint,
    _context_search_tokens,
    _build_readiness_preview,
    _role_preferred_pack_ids,
    _render_source_states,
    handle_capability_overview,
    handle_capability_search,
    handle_capability_state,
)

from ._remote import (  # noqa: F401
    _tokenize_search_text,
    _query_tokens_match,
    _sync_mcp_registry_source,
    _mcp_registry_search_servers,
    _remote_search_mcp_registry_records,
    _js_literal_to_text,
    _skillsmp_proxy_search_url,
    _parse_skillsmp_proxy_search_markdown,
    _skillsmp_api_search_url,
    _parse_skillsmp_api_payload,
    _remote_search_skillsmp_records,
    _clawhub_api_url,
    _clawhub_item_to_record,
    _remote_search_clawhub_records,
    _openclaw_tree_paths,
    _openclaw_frontmatter_for_path,
    _remote_search_openclaw_skill_records,
    _parse_clawskills_data_js,
    _remote_search_clawskills_records,
    _remote_search_skill_records,
    _fetch_mcp_registry_record_by_server_name,
    _normalize_mcp_registry_record,
    _split_frontmatter,
    _parse_frontmatter,
    _extract_skill_capsule,
    _extract_skill_dependencies,
    _validate_agentskill_frontmatter,
    _sync_anthropic_skills_source,
    _mark_source_disabled,
)

from ._install import (  # noqa: F401
    _install_spec_ready,
    _needs_registry_hydration,
    _merge_registry_install_into_record,
    _github_headers,
    _catalog_staleness_seconds,
    _sanitize_tool_token,
    _sanitize_skill_id_token,
    _build_synthetic_tool_name,
    _normalize_mcp_input_schema,
    _normalize_discovered_tools,
    _normalize_registry_argument_entries,
    _normalize_registry_env_names,
    _extract_required_env_from_runtime_arguments,
    _literal_registry_argument_tokens,
    _oci_runtime_argument_tokens,
    _required_environment_names,
    _missing_required_environment_names,
    _command_stdio_command_candidates,
    _package_fallback_command_candidates,
    _preflight_external_install,
    _normalize_registry_type_token,
    _effective_registry_type,
    _tool_name_aliases,
    _is_unknown_tool_error_message,
    _npx_package_command,
    _pypi_package_commands,
    _oci_package_commands,
    _package_stdio_command_candidates,
    _choose_available_command,
    _installer_label_for_command,
    _stdio_mcp_roundtrip,
    _extract_jsonrpc_result,
    _http_jsonrpc_request,
    _remote_mcp_call,
    _supported_external_install_record,
    _record_enable_supported,
    _external_artifact_cache_key,
    _external_artifact_id,
    _is_package_probe_degradable_error,
    _classify_external_install_error,
    _diagnostics_from_install_error,
    _artifact_entry_from_install,
    _upsert_runtime_artifact_for_capability,
    _install_external_capability,
    _invoke_installed_external_tool,
    _invoke_installed_external_tool_with_aliases,
)
from ._handlers import (  # noqa: F401
    _curated_install_metadata,
    _build_curated_records_from_policy,
    _ensure_curated_catalog_records,
    _sync_tier_rank,
    _qualification_rank,
    _catalog_record_sort_key,
    _catalog_max_records,
    _prune_catalog_records,
    _refresh_source_record_counts,
    _sync_catalog,
    _auto_sync_catalog,
    sync_capability_catalog_once,
    _normalize_capability_id_list,
    _normalize_profile_capability_defaults,
    apply_actor_profile_capability_defaults,
    apply_actor_capability_autoload,
    handle_capability_import,
    handle_capability_uninstall,
    handle_capability_tool_call,
)

def try_handle_capability_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "capability_allowlist_get":
        return handle_capability_allowlist_get(args)
    if op == "capability_allowlist_validate":
        return handle_capability_allowlist_validate(args)
    if op == "capability_allowlist_update":
        return handle_capability_allowlist_update(args)
    if op == "capability_allowlist_reset":
        return handle_capability_allowlist_reset(args)
    if op == "capability_overview":
        return handle_capability_overview(args)
    if op == "capability_search":
        return handle_capability_search(args)
    if op == "capability_enable":
        return handle_capability_enable(args)
    if op == "capability_block":
        return handle_capability_block(args)
    if op == "capability_state":
        return handle_capability_state(args)
    if op == "capability_import":
        return handle_capability_import(args)
    if op == "capability_uninstall":
        return handle_capability_uninstall(args)
    if op == "capability_tool_call":
        return handle_capability_tool_call(args)
    return None
