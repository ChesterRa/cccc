pub fn group(value: &str) -> Option<&'static str> {
    Some(match value {
        "create" => "group_create",
        "list" => "groups",
        "get" | "show" => "group_show",
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
pub fn memory(value: &str) -> Option<&'static str> {
    Some(match value {
        "search" => "memory_search",
        "get" | "read" => "memory_get",
        "write" => "memory_write",
        "profile" => "memory_profile_get",
        "health" => "memory_health",
        _ => return None,
    })
}
pub fn memory_admin(value: &str) -> Option<&'static str> {
    Some(match value {
        "layout" => "memory_reme_layout_get",
        "index" | "sync" => "memory_reme_index_sync",
        "compact" => "memory_reme_compact",
        "flush" => "memory_reme_daily_flush",
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
    Some(if value == "ack" {
        "notify_ack"
    } else {
        "system_notify"
    })
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
        "sync" => "group_space_sync",
        "auth" => "group_space_provider_auth",
        _ => return None,
    })
}
pub fn headless(value: &str) -> Option<&'static str> {
    Some(match value {
        "get" | "status" => "headless_status",
        "set" => "headless_set_status",
        "ack" => "headless_ack_message",
        _ => return None,
    })
}
pub fn terminal(value: &str) -> Option<&'static str> {
    Some(match value {
        "tail" => "terminal_tail",
        "history" => "terminal_history",
        "clear" => "terminal_clear",
        "resize" => "terminal_resize",
        _ => return None,
    })
}
pub fn debug(value: &str) -> Option<&'static str> {
    Some(match value {
        "snapshot" => "debug_snapshot",
        "tail" => "debug_tail_logs",
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
        "select" => "assistant_voice_document_select",
        "read" => "assistant_voice_document_input_read",
        "save" => "assistant_voice_document_save",
        "instruction" => "assistant_voice_document_instruction",
        "archive" => "assistant_voice_document_archive",
        _ => return None,
    })
}
pub fn voice_composer(value: &str) -> Option<&'static str> {
    Some(match value {
        "submit" => "assistant_voice_prompt_draft_submit",
        "ack" => "assistant_voice_prompt_draft_ack",
        "feedback" => "assistant_voice_instruction_feedback",
        "clear" => "assistant_voice_ask_requests_clear",
        _ => return None,
    })
}
