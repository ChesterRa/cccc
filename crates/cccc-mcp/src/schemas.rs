use serde_json::{Value, json};

pub fn input(name: &str) -> Value {
    match name {
        "cccc_message_send" | "cccc_tracked_send" => object(messaging(), &["text", "to"]),
        "cccc_message_reply" => object(
            merge(messaging(), json!({"reply_to":{"type":"string"}})),
            &["reply_to", "text"],
        ),
        "cccc_file" => object(
            merge(
                messaging(),
                json!({
                    "action":{"type":"string","enum":["send","blob_path","info","read"]},
                    "path":{"type":"string","description":"File under the active scope."},
                    "rel_path":{"type":"string","description":"Delivered UTF-8 text attachment path."},
                    "max_bytes":{"type":"integer","minimum":1,"maximum":2000000}
                }),
            ),
            &["action"],
        ),
        "cccc_memory" => action(
            &["layout_get", "search", "get", "write"],
            json!({
                "query":{"type":"string"},"target":{"type":"string","enum":["daily","memory"]},
                "content":{"type":"string"},"path":{"type":"string"}
            }),
        ),
        "cccc_memory_admin" => action(
            &["index_sync", "context_check", "compact", "daily_flush"],
            json!({}),
        ),
        "cccc_task" => action(
            &[
                "list", "create", "get", "update", "move", "restore", "archive",
            ],
            json!({
                "task_id":{"type":"string"},"title":{"type":"string"},"status":{"type":"string"},
                "type":{"type":"string","enum":["free","standard","optimization"]},
                "assignee":{"type":"string"},"notes":{"type":"string"}
            }),
        ),
        "cccc_actor" => action(
            &[
                "list",
                "get",
                "add",
                "update",
                "remove",
                "start",
                "stop",
                "restart",
                "new_session",
            ],
            json!({
                "actor_id":{"type":"string"},"runtime":{"type":"string"},
                "runner":{"type":"string","enum":["pty","headless"]},"command":{"type":"array","items":{"type":"string"}}
            }),
        ),
        "cccc_group" => action(
            &[
                "create",
                "list",
                "get",
                "show",
                "update",
                "delete",
                "reset",
                "start",
                "stop",
                "set_state",
                "use",
                "attach",
                "detach_scope",
            ],
            json!({
                "title":{"type":"string"},"topic":{"type":"string"},"state":{"type":"string","enum":["active","idle","paused","stopped"]},
                "path":{"type":"string"},"scope_key":{"type":"string"}
            }),
        ),
        "cccc_terminal" => action(
            &["status", "tail", "history", "write", "resize", "clear"],
            json!({
                "actor_id":{"type":"string"},"data":{"type":"string"},"before":{"oneOf":[{"type":"string"},{"type":"integer"}]},
                "limit_bytes":{"type":"integer"},"cols":{"type":"integer"},"rows":{"type":"integer"}
            }),
        ),
        "cccc_agent_state" => action(
            &["get", "update"],
            json!({
                "actor_id":{"type":"string"},"focus":{"type":"string"},"next_action":{"type":"string"},
                "open_loops":{"type":"array","items":{"type":"string"},"description":"Current memo with concrete referent and exit criteria."},
                "commitments":{"type":"array","items":{"type":"string"},"description":"Promises to users or actors."},
                "persona_notes":{"type":"array","items":{"type":"string"},"description":"Durable collaboration preferences, not temporary task memos."}
            }),
        ),
        "cccc_space" => action(
            &[
                "status",
                "capabilities",
                "bind",
                "ingest",
                "query",
                "sources",
                "artifact",
                "jobs",
                "sync",
                "auth",
            ],
            json!({
                "lane":{"type":"string","enum":["work","memory"]},"query":{"type":"string"},
                "options":{"type":"object","properties":{"source_ids":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}
            }),
        ),
        "cccc_repo" | "cccc_remote_repo" => {
            action(&["info", "list", "list_dir", "read", "search"], repo_read())
        }
        "cccc_repo_edit" | "cccc_remote_repo_edit" => action(
            &[
                "replace",
                "multi_replace",
                "write",
                "mkdir",
                "delete",
                "move",
            ],
            json!({
                "path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},
                "content":{"type":"string"},"replacements":{"type":"array"},"expected_sha256":{"type":"string"}
            }),
        ),
        "cccc_capability_search" => object(
            json!({
                "group_id":{"type":"string"},"query":{"type":"string"},"kind":{"type":"string"},
                "include_external":{"type":"boolean","default":false}
            }),
            &[],
        ),
        "cccc_capability_enable"
        | "cccc_capability_use"
        | "cccc_capability_import"
        | "cccc_capability_block"
        | "cccc_capability_uninstall" => object(
            json!({
                "group_id":{"type":"string"},"capability_id":{"type":"string"},"enabled":{"type":"boolean"},
                "action":{"type":"string"},"args":{"type":"object"}
            }),
            &[],
        ),
        _ => object(common(), &[]),
    }
}

pub fn annotations(name: &str) -> Option<Value> {
    match name {
        "cccc_repo" | "cccc_remote_repo" | "cccc_context_get" | "cccc_inbox_list" => {
            Some(json!({"readOnlyHint":true,"destructiveHint":false}))
        }
        "cccc_repo_edit"
        | "cccc_remote_repo_edit"
        | "cccc_apply_patch"
        | "cccc_remote_apply_patch" => Some(json!({"readOnlyHint":false,"destructiveHint":true})),
        _ => None,
    }
}

fn action(actions: &[&str], extra: Value) -> Value {
    object(
        merge(
            common(),
            merge(json!({"action":{"type":"string","enum":actions}}), extra),
        ),
        &["action"],
    )
}

fn messaging() -> Value {
    merge(
        common(),
        json!({
            "text":{"type":"string"},"to":{"oneOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}]},
            "priority":{"type":"string","enum":["normal","attention"]},"reply_required":{"type":"boolean"},
            "suggested_user_message":{"type":"string","description":"Optional CCCC Web composer hint; not sent automatically and must not be used for approvals."}
        }),
    )
}

fn repo_read() -> Value {
    json!({"path":{"type":"string"},"query":{"type":"string"},"regex":{"type":"boolean"},
        "include_globs":{"type":"array","items":{"type":"string"}},"exclude_globs":{"type":"array","items":{"type":"string"}},
        "context_lines":{"type":"integer"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}})
}

fn common() -> Value {
    json!({"group_id":{"type":"string"},"actor_id":{"type":"string"},"by":{"type":"string"}})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":true})
}

fn merge(left: Value, right: Value) -> Value {
    let mut output = left.as_object().cloned().unwrap_or_default();
    output.extend(right.as_object().cloned().unwrap_or_default());
    Value::Object(output)
}
