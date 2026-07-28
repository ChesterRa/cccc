use serde_json::{Value, json};

pub fn input(name: &str) -> Value {
    match name {
        "cccc_message_send" => object(
            merge(
                messaging(),
                json!({
                    "reply_to":{"type":"string","description":"Optional event ID. Omit for a new message; set it to reply to that message."},
                    "to":{
                        "oneOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}],
                        "description":"Optional recipients. New local messages default to user; replies default to the original sender."
                    }
                }),
            ),
            &["text"],
        ),
        "cccc_tracked_send" => object(messaging(), &["text", "to"]),
        "cccc_message_reply" => object(
            merge(
                messaging(),
                json!({
                    "reply_to":{"type":"string"},
                    "to":{
                        "oneOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}],
                        "description":"Optional reply recipients. Omit to reply to the original sender; never target the current actor."
                    }
                }),
            ),
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
                "content":{"type":"string"},"path":{"type":"string"},
                "max_results":{"type":"integer","minimum":1,"maximum":50},
                "min_score":{"type":"number","minimum":0,"maximum":1},
                "sources":{"type":"array","items":{"type":"string"}},
                "offset":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1,"maximum":5000},
                "date":{"type":"string"},"mode":{"type":"string","enum":["append","replace"]},
                "idempotency_key":{"type":"string"},"source_refs":{"type":"array","items":{"type":"string"}},
                "tags":{"type":"array","items":{"type":"string"}},"supersedes":{"type":"array","items":{"type":"string"}},
                "dedup_intent":{"type":"string","enum":["new","update","supersede","silent"]},"dedup_query":{"type":"string"}
            }),
        ),
        "cccc_memory_admin" => action(
            &["index_sync", "context_check", "compact", "daily_flush"],
            json!({
                "mode":{"type":"string","enum":["scan","rebuild"]},
                "messages":{"type":"array","items":{"type":"object"}},
                "messages_to_summarize":{"type":"array","items":{"type":"object"}},
                "turn_prefix_messages":{"type":"array","items":{"type":"object"}},
                "previous_summary":{"type":"string"},"context_window_tokens":{"type":"integer"},
                "reserve_tokens":{"type":"integer"},"keep_recent_tokens":{"type":"integer"},
                "return_prompt":{"type":"boolean"},"date":{"type":"string"},"language":{"type":"string"},
                "signal_pack":{"type":"object"},"signal_pack_token_budget":{"type":"integer"}
            }),
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
                "profile_list",
                "add",
                "remove",
                "start",
                "stop",
                "restart",
            ],
            json!({
                "actor_id":{"type":"string"},"runtime":{"type":"string"},
                "runner":{"type":"string","enum":["pty","headless"]},"command":{"type":"array","items":{"type":"string"}}
            }),
        ),
        "cccc_group" => action(
            &["info", "list", "resolve", "set_state"],
            json!({
                "token":{"type":"string"},"state":{"type":"string","enum":["active","idle","paused","stopped"]}
            }),
        ),
        "cccc_terminal" => action(
            &["tail"],
            json!({
                "actor_id":{"type":"string"},"target_actor_id":{"type":"string"},
                "max_chars":{"type":"integer","minimum":1,"maximum":100000},
                "strip_ansi":{"type":"boolean"}
            }),
        ),
        "cccc_headless" => action(
            &["status", "set_status", "ack_message"],
            json!({"status":{"type":"string","enum":["idle","working","waiting","stopped"]},
                "task_id":{"type":"string"},"message_id":{"type":"string"}}),
        ),
        "cccc_notify" => action(
            &["send", "ack"],
            json!({"kind":{"type":"string"},"title":{"type":"string"},"message":{"type":"string"},
                "target_actor_id":{"type":"string"},"priority":{"type":"string"},
                "requires_ack":{"type":"boolean"},"notify_event_id":{"type":"string"}}),
        ),
        "cccc_debug" => action(
            &["snapshot", "tail_logs"],
            json!({"component":{"type":"string"},"lines":{"type":"integer"}}),
        ),
        "cccc_voice_secretary_document" => action(
            &["list", "create", "read_new_input", "archive"],
            json!({"document_path":{"type":"string"},"title":{"type":"string"},
                "include_archived":{"type":"boolean"}}),
        ),
        "cccc_voice_secretary_composer" => object(
            merge(
                common(),
                json!({
                    "action":{"type":"string","enum":["submit_prompt_draft"]},
                    "request_id":{"type":"string"},
                    "draft_text":{"type":"string"},
                    "no_op":{"type":"boolean"},
                    "summary":{"type":"string"},
                    "operation":{"type":"string"},
                    "composer_snapshot_hash":{"type":"string"}
                }),
            ),
            &["action", "request_id"],
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
                "provider_auth",
                "provider_credential_status",
                "provider_credential_update",
            ],
            json!({
                "lane":{"type":"string","enum":["work","memory"]},"query":{"type":"string"},
                "provider":{"type":"string"},"sub_action":{"type":"string"},
                "remote_space_id":{"type":"string"},"kind":{"type":"string"},"payload":{"type":"object"},
                "idempotency_key":{"type":"string"},
                "options":{"type":"object","properties":{"source_ids":{"type":"array","items":{"type":"string"}}},"additionalProperties":true},
                "source":{"type":"string"},"source_id":{"type":"string"},"new_title":{"type":"string"},
                "job_id":{"type":"string"},"force":{"type":"boolean"},"wait":{"type":"boolean"},
                "save_to_space":{"type":"boolean"},"output_path":{"type":"string"},"output_format":{"type":"string"},
                "artifact_id":{"type":"string"},"timeout_seconds":{"type":"integer"},
                "auth_json":{"type":"string"},"clear":{"type":"boolean"},"force_reauth":{"type":"boolean"}
            }),
        ),
        "cccc_presentation" => action(
            &["get", "publish", "clear"],
            json!({"slot":{"type":"string"},"card_type":{"type":"string"},"title":{"type":"string"},
                "summary":{"type":"string"},"source_label":{"type":"string"},"source_ref":{"type":"string"},
                "content":{"type":"string"},"table":{"type":"object"},"path":{"type":"string"},
                "url":{"type":"string"},"blob_rel_path":{"type":"string"},"all":{"type":"boolean"}}),
        ),
        "cccc_automation" => action(
            &["state", "manage"],
            json!({"op":{"type":"string"},"actions":{"type":"array","items":{"type":"object"}},
                "expected_version":{"type":"integer"}}),
        ),
        "cccc_im_bind" => object(merge(common(), json!({"key":{"type":"string"}})), &["key"]),
        "cccc_voice_secretary_request" => action(
            &["handoff", "report"],
            json!({"target":{"type":"string"},"request_text":{"type":"string"},"summary":{"type":"string"},
                "request_id":{"type":"string"},"source_request_id":{"type":"string"},
                "status":{"type":"string","enum":["working","done","needs_user","failed"]},
                "reply_text":{"type":"string"},"document_path":{"type":"string"},
                "artifact_paths":{"type":"array","items":{"type":"string"}},
                "source_summary":{"type":"string"},"checked_at":{"type":"string"},
                "source_urls":{"type":"array","items":{"type":"string"}}}),
        ),
        "cccc_remote_access" => action(
            &["list", "status", "explain_permissions"],
            remote(json!({})),
        ),
        "cccc_remote_context" => object(remote(json!({})), &[]),
        "cccc_repo" => action(&["info", "list", "list_dir", "read", "search"], repo_read()),
        "cccc_remote_repo" => action(
            &["info", "list", "list_dir", "read", "search"],
            remote(repo_read()),
        ),
        "cccc_repo_edit" => action(
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
        "cccc_remote_repo_edit" => action(
            &[
                "replace",
                "multi_replace",
                "write",
                "mkdir",
                "delete",
                "move",
            ],
            remote(json!({
                "path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},
                "content":{"type":"string"},"replacements":{"type":"array"},"expected_sha256":{"type":"string"}
            })),
        ),
        "cccc_remote_git" => action(
            &["status", "diff", "log", "add", "commit"],
            remote(
                json!({"path":{"type":"string"},"paths":{"type":"array","items":{"type":"string"}},
                "staged":{"type":"boolean"},"all_changes":{"type":"boolean"},"message":{"type":"string"},
                "count":{"type":"integer"},"max_output_bytes":{"type":"integer"}}),
            ),
        ),
        "cccc_git" => action(
            &["status", "diff", "log", "add", "commit"],
            json!({"path":{"type":"string"},"paths":{"type":"array","items":{"type":"string"}},
                "staged":{"type":"boolean"},"all_changes":{"type":"boolean"},"message":{"type":"string"},
                "count":{"type":"integer"},"max_output_bytes":{"type":"integer"}}),
        ),
        "cccc_remote_apply_patch" => object(remote(json!({"patch":{"type":"string"}})), &["patch"]),
        "cccc_remote_shell" | "cccc_remote_exec_command" => object(
            remote(json!({
                "cmd":{"type":"string"},
                "command":{"oneOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}]},
                "timeout_s":{"type":"integer","minimum":1,"maximum":600}
            })),
            &[],
        ),
        "cccc_remote_write_stdin" => object(
            remote(json!({
                "session_id":{"type":"string"},"chars":{"type":"string"},
                "terminate":{"type":"boolean"},"yield_time_ms":{"type":"integer"}
            })),
            &["session_id"],
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
        "cccc_runtime_complete_turn" => object(
            merge(
                common(),
                json!({
                    "turn_id":{"type":"string","description":"Optional active turn ID. When provided, it must match the current turn."},
                    "status":{"type":"string","enum":["done","partial","failed","cancelled"],"default":"done"},
                    "event_ids":{"type":"array","items":{"type":"string"}},
                    "latest_event_id":{"type":"string"},
                    "summary":{"type":"string"}
                }),
            ),
            &["status"],
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
            "dst_group_id":{"type":"string","description":"Optional local or trusted remote group ID. For a remote Group Bridge route, use its exact remote_group_id and provide an explicit remote recipient."},
            "idempotency_key":{"type":"string","description":"Stable retry key for trusted remote Group Bridge delivery."},
            "refs":{"type":"array","items":{"type":"object"}},
            "insight":{"type":"string","maxLength":cccc_core::peer_insight::INSIGHT_MAX_CHARS,"description":cccc_core::peer_insight::PEER_INSIGHT_FIELD_DESCRIPTION},
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

fn remote(extra: Value) -> Value {
    merge(
        json!({"remote_group_id":{"type":"string","description":"Target trusted remote group ID."}}),
        extra,
    )
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":true})
}

fn merge(left: Value, right: Value) -> Value {
    let mut output = left.as_object().cloned().unwrap_or_default();
    output.extend(right.as_object().cloned().unwrap_or_default());
    Value::Object(output)
}

#[cfg(test)]
mod tests {
    use super::input;
    use serde_json::json;

    #[test]
    fn messaging_schema_exposes_peer_insight_contract() {
        for tool in [
            "cccc_message_send",
            "cccc_tracked_send",
            "cccc_message_reply",
            "cccc_file",
        ] {
            let schema = input(tool);
            assert_eq!(
                schema["properties"]["insight"]["maxLength"],
                cccc_core::peer_insight::INSIGHT_MAX_CHARS
            );
            assert_eq!(
                schema["properties"]["insight"]["description"],
                cccc_core::peer_insight::PEER_INSIGHT_FIELD_DESCRIPTION
            );
        }
    }

    #[test]
    fn message_schema_explains_new_and_reply_defaults() {
        let schema = input("cccc_message_send");
        let description = schema["properties"]["to"]["description"]
            .as_str()
            .expect("recipient description");
        assert!(description.contains("original sender"));
        assert!(description.contains("default to user"));
        assert!(
            schema["properties"]["reply_to"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("Omit for a new message"))
        );
        assert!(
            schema["properties"]["dst_group_id"]["description"]
                .as_str()
                .is_some_and(|description| {
                    description.contains("trusted remote")
                        && description.contains("explicit remote recipient")
                })
        );
        assert_eq!(schema["properties"]["idempotency_key"]["type"], "string");
    }

    #[test]
    fn complete_turn_schema_exposes_optional_turn_id() {
        let schema = input("cccc_runtime_complete_turn");
        assert_eq!(schema["properties"]["turn_id"]["type"], "string");
        assert!(
            schema["required"]
                .as_array()
                .is_some_and(|required| !required.iter().any(|item| item == "turn_id"))
        );
        assert!(
            schema["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|item| item == "status"))
        );
        assert!(
            schema["required"]
                .as_array()
                .is_some_and(|required| !required.iter().any(|item| item == "event_ids"))
        );
    }

    #[test]
    fn terminal_schema_exposes_transcript_rendering_options() {
        let schema = input("cccc_terminal");
        assert_eq!(schema["properties"]["action"]["enum"], json!(["tail"]));
        assert_eq!(schema["properties"]["strip_ansi"]["type"], "boolean");
        assert_eq!(schema["properties"]["max_chars"]["type"], "integer");
    }

    #[test]
    fn voice_document_schema_matches_python_actions() {
        let schema = input("cccc_voice_secretary_document");
        assert_eq!(
            schema["properties"]["action"]["enum"],
            json!(["list", "create", "read_new_input", "archive"])
        );
    }

    #[test]
    fn voice_composer_schema_requires_python_request_identity() {
        let schema = input("cccc_voice_secretary_composer");
        assert_eq!(
            schema["properties"]["action"]["enum"],
            json!(["submit_prompt_draft"])
        );
        assert_eq!(schema["properties"]["draft_text"]["type"], "string");
        assert!(
            schema["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|item| item == "request_id"))
        );
    }
}
