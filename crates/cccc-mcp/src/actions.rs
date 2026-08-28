pub fn group(value: &str) -> Option<&'static str> {
    Some(match value {
        "create" => "group_create",
        "list" => "groups",
        "info" | "get" | "show" => "group_show",
        "resolve" => "group_resolve",
        "update" => "group_update",
        "delete" => "group_delete",
        "reset" => "group_reset",
        "start" => "group_start",
        "stop" => "group_stop",
        "set_state" => "group_set_state",
        "use" => "group_use",
        "attach" => "attach",
        "detach_scope" => "group_detach_scope",
        _ => return None,
    })
}
pub fn actor(value: &str) -> Option<&'static str> {
    Some(match value {
        "list" | "get" => "actor_list",
        "profile_list" => "actor_profile_list",
        "add" => "actor_add",
        "update" => "actor_update",
        "remove" => "actor_remove",
        "start" => "actor_start",
        "stop" => "actor_stop",
        "restart" => "actor_restart",
        "new_session" => "actor_new_session",
        _ => return None,
    })
}
pub fn actor_notes(value: &str) -> Option<&'static str> {
    Some(match value {
        "get" => "actor_notes_get",
        "set" => "actor_notes_set",
        "clear" => "actor_notes_clear",
        _ => return None,
    })
}
pub fn memory(value: &str) -> Option<&'static str> {
    Some(match value {
        "layout_get" => "memory_reme_layout_get",
        "search" => "memory_reme_search",
        "get" | "read" => "memory_reme_get",
        "write" => "memory_reme_write",
        "profile" => "memory_profile_get",
        "health" => "memory_health",
        _ => return None,
    })
}
pub fn memory_admin(value: &str) -> Option<&'static str> {
    Some(match value {
        "index_sync" | "index" | "sync" => "memory_reme_index_sync",
        "context_check" => "memory_reme_context_check",
        "compact" => "memory_reme_compact",
        "daily_flush" | "flush" => "memory_reme_daily_flush",
        _ => return None,
    })
}
pub fn automation(value: &str) -> Option<&'static str> {
    Some(match value {
        "get" | "state" => "group_automation_state",
        "update" => "group_automation_update",
        "manage" => "group_automation_manage",
        "reset" => "group_automation_reset_baseline",
        _ => return None,
    })
}
pub fn notify(value: &str) -> Option<&'static str> {
    (value == "send").then_some("system_notify")
}
pub fn presentation(value: &str) -> Option<&'static str> {
    Some(match value {
        "get" | "list" => "presentation_get",
        "publish" => "presentation_publish",
        "clear" => "presentation_clear",
        _ => return None,
    })
}
pub fn space(value: &str) -> Option<&'static str> {
    Some(match value {
        "status" => "group_space_status",
        "capabilities" => "group_space_capabilities",
        "bind" => "group_space_bind",
        "ingest" => "group_space_ingest",
        "query" => "group_space_query",
        "sources" => "group_space_sources",
        "artifact" => "group_space_artifact",
        "jobs" => "group_space_jobs",
        "provider_auth" | "auth" => "group_space_provider_auth",
        "provider_credential_status" => "group_space_provider_credential_status",
        "provider_credential_update" => "group_space_provider_credential_update",
        _ => return None,
    })
}
pub fn headless(value: &str) -> Option<&'static str> {
    Some(match value {
        "get" | "status" => "headless_status",
        "set" | "set_status" => "headless_set_status",
        _ => return None,
    })
}
pub fn terminal(value: &str) -> Option<&'static str> {
    Some(match value {
        "tail" => "terminal_tail",
        "history" => "terminal_history",
        "clear" => "terminal_clear",
        "resize" => "term_resize",
        _ => return None,
    })
}

pub fn debug(value: &str) -> Option<&'static str> {
    Some(match value {
        "snapshot" => "debug_snapshot",
        "tail" | "tail_logs" => "debug_tail_logs",
        "clear" => "debug_clear_logs",
        _ => return None,
    })
}
pub fn im(value: &str) -> Option<&'static str> {
    Some(match value {
        "bind" => "im_bind_chat",
        "pending" => "im_list_pending",
        "authorized" | "list" => "im_list_authorized",
        "reject" => "im_reject_pending",
        "revoke" => "im_revoke_chat",
        _ => return None,
    })
}
pub fn voice_document(value: &str) -> Option<&'static str> {
    Some(match value {
        "list" => "assistant_voice_document_list",
        "create" => "assistant_voice_document_save",
        "read_new_input" => "assistant_voice_document_input_read",
        "archive" => "assistant_voice_document_archive",
        _ => return None,
    })
}
pub fn voice_composer(value: &str) -> Option<&'static str> {
    Some(match value {
        "submit_prompt_draft" => "assistant_voice_prompt_draft_submit",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn notify_accepts_only_the_current_send_action() {
        assert_eq!(super::notify("send"), Some("system_notify"));
        assert_eq!(super::notify("ack"), None);
        assert_eq!(super::notify("unknown"), None);
    }
}
