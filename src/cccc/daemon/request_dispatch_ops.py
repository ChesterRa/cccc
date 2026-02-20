"""Daemon request dispatch orchestration."""

from __future__ import annotations

from pathlib import Path
from dataclasses import dataclass
from typing import Any, Callable, Type

from ..contracts.v1 import DaemonRequest, DaemonResponse
from .ops.context_ops import try_handle_context_op
from .ops.actor_ops import try_handle_actor_aux_op
from .ops.actor_profile_ops import try_handle_actor_profile_op
from .ops.actor_add_ops import try_handle_actor_add_op
from .ops.actor_lifecycle_ops import try_handle_actor_lifecycle_op
from .ops.actor_membership_ops import try_handle_actor_membership_op
from .ops.actor_update_ops import try_handle_actor_update_op
from .ops.inbox_ack_ops import try_handle_inbox_ack_op
from .ops.inbox_read_ops import try_handle_inbox_read_op
from .ops.maintenance_ops import try_handle_maintenance_op
from .ops.diagnostics_ops import try_handle_diagnostics_op
from .ops.daemon_core_ops import try_handle_daemon_core_op
from .ops.remote_access_ops import try_handle_remote_access_op
from .ops.chat_ops import try_handle_chat_op
from .ops.system_notify_ops import try_handle_system_notify_op
from .ops.group_state_ops import try_handle_group_state_op
from .ops.group_lifecycle_ops import try_handle_group_lifecycle_op
from .ops.automation_ops import try_handle_group_automation_op
from .ops.group_settings_ops import try_handle_group_settings_op
from .ops.group_ops import try_handle_group_core_op
from .ops.group_bootstrap_ops import try_handle_group_bootstrap_op
from .ops.registry_ops import try_handle_registry_op
from .ops.im_ops import try_handle_im_op
from .ops.runner_ops import try_handle_headless_op


@dataclass(frozen=True)
class RequestDispatchDeps:
    version: str
    pid_provider: Callable[[], int]
    now_iso: Callable[[], str]
    get_observability: Callable[[], dict[str, Any]]
    update_observability_settings: Callable[..., bool]
    apply_observability_settings: Callable[[dict[str, Any]], bool]
    developer_mode_enabled: Callable[[], bool]
    effective_runner_kind: Callable[[str], str]
    throttle_debug_summary: Callable[[], dict[str, Any]]
    can_read_terminal_transcript: Callable[..., bool]
    pty_backlog_bytes: Callable[[], int]
    group_create_from_template: Callable[..., DaemonResponse]
    group_template_export: Callable[..., DaemonResponse]
    group_template_preview: Callable[..., DaemonResponse]
    group_template_import_replace: Callable[..., DaemonResponse]
    foreman_id: Callable[[Any], str]
    maybe_reset_automation_on_foreman_change: Callable[..., None]
    stop_im_bridges_for_group: Callable[[str], None]
    delete_group_private_env: Callable[[str], None]
    pty_supported: Callable[[], bool]
    find_scope_url: Callable[[Any, str], str]
    ensure_mcp_installed: Callable[..., bool]
    merge_actor_env_with_private: Callable[..., dict[str, str]]
    inject_actor_context_env: Callable[[dict[str, str], str, str], dict[str, str]]
    normalize_runtime_command: Callable[..., list[str]]
    prepare_pty_env: Callable[[dict[str, str]], dict[str, str]]
    write_headless_state: Callable[..., None]
    write_pty_state: Callable[..., None]
    clear_preamble_sent: Callable[[str, str], None]
    throttle_reset_actor: Callable[..., None]
    reset_automation_timers_if_active: Callable[[Any], None]
    supported_runtimes: set[str]
    pty_state_dir_for_group: Callable[[str], Path]
    headless_state_dir_for_group: Callable[[str], Path]
    automation_on_resume: Callable[[Any], None]
    clear_pending_system_notifies: Callable[[str], None]
    load_actor_private_env: Callable[[str, str], dict[str, str]]
    validate_private_env_key: Callable[[Any], str]
    coerce_private_env_value: Callable[[Any], str]
    update_actor_private_env: Callable[..., dict[str, str]]
    private_env_max_keys: int
    start_actor_process: Callable[..., dict[str, Any]]
    delete_actor_private_env: Callable[[str, str], None]
    get_actor_profile: Callable[[str], dict[str, Any] | None]
    load_actor_profile_secrets: Callable[[str], dict[str, str]]
    warn_forced_headless: Callable[[str, str], None]
    remove_headless_state: Callable[[str, str], None]
    remove_pty_state_if_pid: Callable[..., None]
    throttle_clear_actor: Callable[[str, str], None]
    daemon_request_factory: Type[DaemonRequest]
    coerce_bool_default_false: Callable[[Any], bool]
    normalize_attachments: Callable[..., list[dict[str, Any]]]
    auto_wake_recipients: Callable[..., None]
    automation_on_new_message: Callable[[Any], None]
    clear_pending_system_notifies_chat: Callable[[str, set[str]], None]
    queue_system_notify: Callable[..., None]
    error_factory: Callable[[str, str], DaemonResponse]


def dispatch_request(
    req: DaemonRequest,
    *,
    deps: RequestDispatchDeps,
    recurse: Any,
) -> tuple[DaemonResponse, bool]:
    op = str(req.op or "").strip()
    args = req.args or {}

    daemon_core_resp = try_handle_daemon_core_op(
        op,
        args,
        version=deps.version,
        pid_provider=deps.pid_provider,
        now_iso=deps.now_iso,
        get_observability=deps.get_observability,
        update_observability_settings=deps.update_observability_settings,
        apply_observability_settings=deps.apply_observability_settings,
    )
    if daemon_core_resp is not None:
        return daemon_core_resp

    remote_access_resp = try_handle_remote_access_op(op, args)
    if remote_access_resp is not None:
        return remote_access_resp, False

    diagnostics_resp = try_handle_diagnostics_op(
        op,
        args,
        developer_mode_enabled=deps.developer_mode_enabled,
        get_observability=deps.get_observability,
        effective_runner_kind=deps.effective_runner_kind,
        throttle_debug_summary=deps.throttle_debug_summary,
        can_read_terminal_transcript=deps.can_read_terminal_transcript,
        pty_backlog_bytes=deps.pty_backlog_bytes,
    )
    if diagnostics_resp is not None:
        return diagnostics_resp, False

    group_bootstrap_resp = try_handle_group_bootstrap_op(
        op,
        args,
        group_create_from_template=deps.group_create_from_template,
        group_template_export=deps.group_template_export,
        group_template_preview=deps.group_template_preview,
        group_template_import_replace=deps.group_template_import_replace,
        foreman_id=deps.foreman_id,
        maybe_reset_automation_on_foreman_change=deps.maybe_reset_automation_on_foreman_change,
    )
    if group_bootstrap_resp is not None:
        return group_bootstrap_resp, False

    group_core_resp = try_handle_group_core_op(
        op,
        args,
        stop_im_bridges_for_group=deps.stop_im_bridges_for_group,
        delete_group_private_env=deps.delete_group_private_env,
    )
    if group_core_resp is not None:
        return group_core_resp, False

    group_settings_resp = try_handle_group_settings_op(op, args)
    if group_settings_resp is not None:
        return group_settings_resp, False

    automation_resp = try_handle_group_automation_op(op, args)
    if automation_resp is not None:
        return automation_resp, False

    registry_resp = try_handle_registry_op(op, args)
    if registry_resp is not None:
        return registry_resp, False

    group_lifecycle_resp = try_handle_group_lifecycle_op(
        op,
        args,
        pty_supported=deps.pty_supported,
        effective_runner_kind=deps.effective_runner_kind,
        find_scope_url=deps.find_scope_url,
        ensure_mcp_installed=deps.ensure_mcp_installed,
        merge_actor_env_with_private=deps.merge_actor_env_with_private,
        inject_actor_context_env=deps.inject_actor_context_env,
        normalize_runtime_command=deps.normalize_runtime_command,
        prepare_pty_env=deps.prepare_pty_env,
        pty_backlog_bytes=deps.pty_backlog_bytes,
        write_headless_state=deps.write_headless_state,
        write_pty_state=deps.write_pty_state,
        clear_preamble_sent=deps.clear_preamble_sent,
        throttle_reset_actor=deps.throttle_reset_actor,
        reset_automation_timers_if_active=deps.reset_automation_timers_if_active,
        supported_runtimes=deps.supported_runtimes,
        pty_state_dir_for_group=deps.pty_state_dir_for_group,
        headless_state_dir_for_group=deps.headless_state_dir_for_group,
        get_actor_profile=deps.get_actor_profile,
        load_actor_profile_secrets=deps.load_actor_profile_secrets,
        update_actor_private_env=deps.update_actor_private_env,
    )
    if group_lifecycle_resp is not None:
        return group_lifecycle_resp, False

    group_state_resp = try_handle_group_state_op(
        op,
        args,
        automation_on_resume=deps.automation_on_resume,
        clear_pending_system_notifies=deps.clear_pending_system_notifies,
    )
    if group_state_resp is not None:
        return group_state_resp, False

    actor_aux_resp = try_handle_actor_aux_op(
        op,
        args,
        effective_runner_kind=deps.effective_runner_kind,
        load_actor_private_env=deps.load_actor_private_env,
        validate_private_env_key=deps.validate_private_env_key,
        coerce_private_env_value=deps.coerce_private_env_value,
        update_actor_private_env=deps.update_actor_private_env,
        private_env_max_keys=deps.private_env_max_keys,
    )
    if actor_aux_resp is not None:
        return actor_aux_resp, False

    actor_profile_resp = try_handle_actor_profile_op(op, args)
    if actor_profile_resp is not None:
        return actor_profile_resp, False

    actor_add_resp = try_handle_actor_add_op(
        op,
        args,
        foreman_id=deps.foreman_id,
        maybe_reset_automation_on_foreman_change=deps.maybe_reset_automation_on_foreman_change,
        start_actor_process=deps.start_actor_process,
        pty_supported=deps.pty_supported,
        effective_runner_kind=deps.effective_runner_kind,
        validate_private_env_key=deps.validate_private_env_key,
        coerce_private_env_value=deps.coerce_private_env_value,
        update_actor_private_env=deps.update_actor_private_env,
        delete_actor_private_env=deps.delete_actor_private_env,
        private_env_max_keys=deps.private_env_max_keys,
        supported_runtimes=deps.supported_runtimes,
        get_actor_profile=deps.get_actor_profile,
        load_actor_profile_secrets=deps.load_actor_profile_secrets,
        warn_forced_headless=deps.warn_forced_headless,
    )
    if actor_add_resp is not None:
        return actor_add_resp, False

    actor_membership_resp = try_handle_actor_membership_op(
        op,
        args,
        foreman_id=deps.foreman_id,
        maybe_reset_automation_on_foreman_change=deps.maybe_reset_automation_on_foreman_change,
        remove_headless_state=deps.remove_headless_state,
        remove_pty_state_if_pid=deps.remove_pty_state_if_pid,
        throttle_clear_actor=deps.throttle_clear_actor,
        delete_actor_private_env=deps.delete_actor_private_env,
    )
    if actor_membership_resp is not None:
        return actor_membership_resp, False

    actor_update_resp = try_handle_actor_update_op(
        op,
        args,
        foreman_id=deps.foreman_id,
        maybe_reset_automation_on_foreman_change=deps.maybe_reset_automation_on_foreman_change,
        find_scope_url=deps.find_scope_url,
        effective_runner_kind=deps.effective_runner_kind,
        ensure_mcp_installed=deps.ensure_mcp_installed,
        merge_actor_env_with_private=deps.merge_actor_env_with_private,
        inject_actor_context_env=deps.inject_actor_context_env,
        normalize_runtime_command=deps.normalize_runtime_command,
        prepare_pty_env=deps.prepare_pty_env,
        pty_backlog_bytes=deps.pty_backlog_bytes,
        write_headless_state=deps.write_headless_state,
        write_pty_state=deps.write_pty_state,
        clear_preamble_sent=deps.clear_preamble_sent,
        throttle_reset_actor=deps.throttle_reset_actor,
        remove_headless_state=deps.remove_headless_state,
        remove_pty_state_if_pid=deps.remove_pty_state_if_pid,
        supported_runtimes=deps.supported_runtimes,
        get_actor_profile=deps.get_actor_profile,
        load_actor_profile_secrets=deps.load_actor_profile_secrets,
        update_actor_private_env=deps.update_actor_private_env,
    )
    if actor_update_resp is not None:
        return actor_update_resp, False

    actor_lifecycle_resp = try_handle_actor_lifecycle_op(
        op,
        args,
        foreman_id=deps.foreman_id,
        maybe_reset_automation_on_foreman_change=deps.maybe_reset_automation_on_foreman_change,
        start_actor_process=deps.start_actor_process,
        effective_runner_kind=deps.effective_runner_kind,
        pty_supported=deps.pty_supported,
        get_actor_profile=deps.get_actor_profile,
        load_actor_profile_secrets=deps.load_actor_profile_secrets,
        update_actor_private_env=deps.update_actor_private_env,
        remove_headless_state=deps.remove_headless_state,
        remove_pty_state_if_pid=deps.remove_pty_state_if_pid,
        clear_preamble_sent=deps.clear_preamble_sent,
        throttle_reset_actor=deps.throttle_reset_actor,
        find_scope_url=deps.find_scope_url,
        merge_actor_env_with_private=deps.merge_actor_env_with_private,
        inject_actor_context_env=deps.inject_actor_context_env,
        normalize_runtime_command=deps.normalize_runtime_command,
        prepare_pty_env=deps.prepare_pty_env,
        pty_backlog_bytes=deps.pty_backlog_bytes,
        write_headless_state=deps.write_headless_state,
        write_pty_state=deps.write_pty_state,
    )
    if actor_lifecycle_resp is not None:
        return actor_lifecycle_resp, False

    inbox_read_resp = try_handle_inbox_read_op(op, args)
    if inbox_read_resp is not None:
        return inbox_read_resp, False

    inbox_ack_resp = try_handle_inbox_ack_op(op, args)
    if inbox_ack_resp is not None:
        return inbox_ack_resp, False

    maintenance_resp = try_handle_maintenance_op(
        op,
        args,
        dispatch_send=lambda relay_op, relay_args: recurse(
            deps.daemon_request_factory(op=relay_op, args=relay_args)
        ),
    )
    if maintenance_resp is not None:
        return maintenance_resp, False

    chat_resp = try_handle_chat_op(
        op,
        args,
        coerce_bool=deps.coerce_bool_default_false,
        normalize_attachments=deps.normalize_attachments,
        effective_runner_kind=deps.effective_runner_kind,
        auto_wake_recipients=deps.auto_wake_recipients,
        automation_on_resume=deps.automation_on_resume,
        automation_on_new_message=deps.automation_on_new_message,
        clear_pending_system_notifies=deps.clear_pending_system_notifies_chat,
    )
    if chat_resp is not None:
        return chat_resp, False

    context_resp = try_handle_context_op(op, args)
    if context_resp is not None:
        return context_resp, False

    im_resp = try_handle_im_op(op, args)
    if im_resp is not None:
        return im_resp, False

    headless_resp = try_handle_headless_op(op, args)
    if headless_resp is not None:
        return headless_resp, False

    system_notify_resp = try_handle_system_notify_op(
        op,
        args,
        coerce_bool=deps.coerce_bool_default_false,
        queue_system_notify=deps.queue_system_notify,
    )
    if system_notify_resp is not None:
        return system_notify_resp, False

    return deps.error_factory("unknown_op", f"unknown op: {op}"), False
