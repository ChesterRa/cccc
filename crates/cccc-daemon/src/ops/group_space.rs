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
    let provider_state = provider_runtime_state(home, &provider)?;
    let sync = sync::work_status_value(home, &group_id, &value)?;
    let (_, memory_sync) = sync::memory_status_values(home, &group_id, &provider, &value)?;
    object(json!({
        "group_id":group_id,
        "provider":provider_state,
        "bindings":value["bindings"],
        "queue_summary":{"work":summary_for(&value,"work"),"memory":summary_for(&value,"memory")},
        "sync":sync,
        "memory_sync":memory_sync
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
            "Native Rust reads Python-compatible work and memory sync status but does not mutate remote sync state.",
            "Native Rust wait=false returns after remote generation starts; automatic background save is not yet available.",
            "Native Rust artifact download currently supports audio, video, report/study guide, infographic, and slide deck outputs.",
            "NotebookLM uses an unofficial upstream protocol and may require compatibility updates."
        ],
        "capabilities":json!(["bind","ingest","query","sources","artifact","jobs"]),
        "unavailable_capabilities":json!(["sync.work","sync.memory","resource_ingest.file","resource_ingest.web_page","resource_ingest.youtube","resource_ingest.google_drive","artifact.download.quiz","artifact.download.flashcards","artifact.download.mind_map","artifact.download.data_table"]),
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
