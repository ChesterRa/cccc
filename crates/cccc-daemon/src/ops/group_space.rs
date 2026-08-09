use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::space_credentials;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use std::io;
use std::path::Path;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};

mod artifacts;
mod notebooklm;
mod operations;
mod provider_ops;
mod state;
mod sync;

use state::*;

const MAX_LOCAL_FILE_SIZE_BYTES: u64 = 200 * 1024 * 1024;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_space_status" => status(home, request),
        "group_space_capabilities" => capabilities(home, request),
        "group_space_bind" => bind(home, request),
        "group_space_ingest" => operations::ingest(home, request),
        "group_space_query" => operations::query(home, request),
        "group_space_sources" => operations::sources(home, request),
        "group_space_artifact" => operations::artifact(home, request),
        "group_space_jobs" => operations::jobs(home, request),
        "group_space_sync" => operations::sync(home, request),
        "group_space_provider_credential_status" => provider_ops::credential_status(home, request),
        "group_space_provider_credential_update" => provider_ops::credential_update(home, request),
        "group_space_provider_health_check" => provider_ops::provider_health(home, request),
        "group_space_spaces" => provider_ops::spaces(home, request),
        "group_space_provider_auth" => provider_ops::provider_auth(home, request),
        _ => return None,
    })
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let value = load(home, &group_id)?;
    let auth_configured =
        space_credentials::status(home, &provider).map_err(OpError::io)?["configured"]
            .as_bool()
            .unwrap_or(false);
    let provider_record = provider_record(home, &provider)?;
    let enabled = provider_record["enabled"].as_bool().unwrap_or(false);
    let real_enabled = provider_record["real_enabled"].as_bool().unwrap_or(false);
    let mode = provider_record["mode"].as_str().unwrap_or("disabled");
    let write_ready = auth_configured && enabled && real_enabled && mode == "active";
    object(json!({
        "group_id":group_id,
        "provider":{"provider":provider,"enabled":enabled,"real_enabled":real_enabled,"real_adapter_enabled":real_enabled,"auth_configured":auth_configured,"mode":mode,"write_ready":write_ready,"readiness_reason":if !auth_configured{"credential missing"}else if write_ready{"ready"}else{"provider disabled"},"last_health_at":provider_record["last_health_at"],"last_error":provider_record["last_error"]},
        "bindings":value["bindings"],
        "queue_summary":{"work":summary_for(&value,"work"),"memory":summary_for(&value,"memory")},
        "sync":value.get("sync").cloned().unwrap_or(json!({"available":false,"converged":false,"reason":"provider_unavailable"})),
        "memory_sync":{
            "lane":"memory",
            "manifest_path":"",
            "last_scan_at":null,
            "last_success_at":null,
            "pending_files":0,
            "running_files":0,
            "failed_files":0,
            "blocked_files":0,
            "eligible_daily_files":0,
            "synced_daily_files":0,
            "empty_daily_skipped":0,
            "last_eligible_daily_date":null,
            "last_synced_daily_date":null
        }
    }))
}
fn capabilities(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(&group_id))
        .map_err(OpError::io)?;
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first());
    let space_root = scope
        .map(|scope| Path::new(&scope.url).join("space"))
        .unwrap_or_default();
    object(json!({
        "group_id":group_id,
        "provider":provider,
        "local_scope_attached":scope.is_some(),
        "space_root":space_root,
        "local_file_policy":{
            "allowed_extensions":[".md",".txt"],
            "max_file_size_bytes":MAX_LOCAL_FILE_SIZE_BYTES,
            "unsupported_error_code":"space_source_unsupported_format",
            "oversize_error_code":"space_source_file_too_large"
        },
        "ingest":{
            "kinds":["context_sync","resource_ingest"],
            "resource_ingest":{
                "source_types":["pasted_text"],
                "required_fields":{"pasted_text":["source_type","content"]},
                "optional_fields":{"pasted_text":["title"]},
                "aliases":{"text":"pasted_text"},
                "examples":{"pasted_text":{"source_type":"pasted_text","content":"Design notes..."}}
            }
        },
        "query":{
            "options":{"source_ids":"Optional remote source_id list to constrain retrieval scope"},
            "unsupported_options":{
                "language":"Not supported by NotebookLM query API. Put language requirements in query text.",
                "lang":"Alias of language; also unsupported for query."
            },
            "examples":{
                "basic":{"query":"Summarize key decisions from the notebook."},
                "scoped":{"query":"Summarize only this source.","options":{"source_ids":["src_123"]}}
            }
        },
        "artifacts":{
            "actions":["list","generate","download"],
            "kinds":["audio","video","report","study_guide","quiz","flashcards","infographic","slide_deck","data_table","mind_map"],
            "options":{
                "language":"Preferred output language",
                "instructions":"Provider-side generation instructions",
                "source_ids":"Optional remote source_id list to constrain generation scope"
            },
            "aliases":{"slide":"slide_deck","slides":"slide_deck","deck":"slide_deck","study":"study_guide"},
            "examples":{"generate_audio":{"action":"generate","kind":"audio","wait":true,"save_to_space":true}}
        },
        "notes":[
            "Native Rust resource_ingest currently supports pasted_text only; unsupported source types fail explicitly.",
            "Work and memory file sync upload .md/.txt content as pasted text.",
            "Native Rust wait=false returns after remote generation starts; automatic background save is not yet available.",
            "Native Rust artifact download currently supports audio, video, report/study guide, infographic, and slide deck outputs.",
            "NotebookLM uses an unofficial upstream protocol and may require compatibility updates."
        ],
        "capabilities":json!(["bind","ingest","query","sources","artifact","jobs","sync"]),
        "unavailable_capabilities":json!(["resource_ingest.file","resource_ingest.web_page","resource_ingest.youtube","resource_ingest.google_drive","artifact.download.quiz","artifact.download.flashcards","artifact.download.mind_map","artifact.download.data_table"]),
        "mode":"remote"
    }))
}
fn bind(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let action = string_arg(request, "action").unwrap_or_else(|| "bind".into());
    let mut remote = string_arg(request, "remote_space_id").unwrap_or_default();
    if action != "unbind" && provider == "notebooklm" && remote.is_empty() {
        let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
        let group = store.load(&group_id).map_err(OpError::io)?;
        remote = notebooklm::create_notebook(home, &format!("{} - {}", group.title, lane))?.id;
    }
    update(home, &group_id, |value| {
        let bindings = map_field(root(value), "bindings");
        if action == "unbind" {
            bindings.insert(
                lane.clone(),
                json!({
                    "group_id":group_id,
                    "provider":provider,
                    "lane":lane,
                    "remote_space_id":"",
                    "bound_by":string_arg(request, "by").unwrap_or_else(|| "user".into()),
                    "bound_at":utc_now(),
                    "status":"unbound"
                }),
            );
        } else {
            bindings.insert(
                lane.clone(),
                json!({
                    "group_id":group_id,
                    "provider":provider,
                    "lane":lane,
                    "remote_space_id":remote,
                    "bound_by":string_arg(request, "by").unwrap_or_else(|| "user".into()),
                    "status":"bound",
                    "bound_at":utc_now()
                }),
            );
        }
        Ok(())
    })?;
    status(home, request)
}
