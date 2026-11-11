use super::*;
use cccc_core::{GroupStore, Scope};
use serde_json::Map;
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    home: HomeLayout,
    group: GroupDoc,
    root: std::path::PathBuf,
}

#[test]
fn empty_argument_compatibility_probes_do_not_create_or_change_a_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    for op in [
        "backlog_get",
        "backlog_use",
        "backlog_create",
        "backlog_exclude",
        "backlog_add",
        "backlog_edit",
        "backlog_move",
        "backlog_close",
        "backlog_promote",
    ] {
        let request = DaemonRequest {
            v: 1,
            op: op.into(),
            args: Map::new(),
        };
        let error = resolve_operation(&request)
            .expect("operation")
            .execute(&home, &request)
            .expect_err("group required");
        assert_eq!(error.code, "invalid_args", "{op}");
    }
    assert!(!temp.path().join("groups").exists());
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("valid test fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("valid test fixture");
        let root = temp.path().join("workspace");
        std::fs::create_dir(&root).expect("valid test fixture");
        let store = GroupStore::new(home.clone()).expect("valid test fixture");
        let mut group = store.create("backlog", "").expect("valid test fixture");
        group.active_scope_key = "s_test".into();
        group.scopes.push(Scope {
            scope_key: "s_test".into(),
            url: root.to_str().expect("valid test fixture").into(),
            label: String::new(),
            git_remote: String::new(),
        });
        for id in ["alpha", "beta"] {
            cccc_core::actors::add(
                &mut group,
                serde_json::from_value(json!({"id":id,"runtime":"custom"}))
                    .expect("valid test fixture"),
            )
            .expect("valid test fixture");
        }
        store.save(&group).expect("valid test fixture");
        Self {
            _temp: temp,
            home,
            group,
            root,
        }
    }

    fn call(&self, op: &str, args: Value) -> OpResult {
        let mut args = args.as_object().cloned().expect("valid test fixture");
        args.entry("by").or_insert(json!("user"));
        args.insert("group_id".into(), json!(self.group.group_id));
        args.entry("scope_key").or_insert(json!("s_test"));
        args.entry("scope_url")
            .or_insert(json!(self.root.to_str().expect("valid test fixture")));
        let request = DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        };
        crate::dispatch::resolve_operation(&request)
            .expect("valid test fixture")
            .execute(&self.home, &request)
    }

    fn create(&self) -> Map<String, Value> {
        self.call(
            "backlog_create",
            json!({"path":"BACKLOG.md","keep_local":false}),
        )
        .expect("valid test fixture")
    }
}

#[test]
fn empty_state_is_read_only_and_existing_document_is_never_reformatted_on_selection() {
    let f = Fixture::new();
    assert_eq!(
        f.call("backlog_get", json!({}))
            .expect("valid test fixture")["configured"],
        false
    );
    assert!(!f.root.join("BACKLOG.md").exists());
    let content = "# 路线\r\n## 今后\r\n### A\r\nBody without final newline";
    std::fs::write(f.root.join("BACKLOG.md"), content).expect("valid test fixture");
    let result = f
        .call("backlog_use", json!({"path":"BACKLOG.md"}))
        .expect("valid test fixture");
    assert_eq!(result["sections"][1]["entries"][0]["id"], Value::Null);
    assert_eq!(
        std::fs::read(f.root.join("BACKLOG.md")).expect("valid test fixture"),
        content.as_bytes()
    );
    assert_eq!(
        f.call("backlog_create", json!({"path":"BACKLOG.md"}))
            .expect_err("operation is rejected")
            .code,
        "backlog_file_conflict"
    );
    std::fs::remove_file(f.root.join("BACKLOG.md")).expect("valid test fixture");
    let result = f
        .call("backlog_get", json!({}))
        .expect("valid test fixture");
    assert_eq!(result["configured"], true);
    assert_eq!(result["exists"], false);
}

#[test]
fn entry_flow_keeps_task_history_deduplicates_promote_and_requires_explicit_close() {
    let f = Fixture::new();
    let empty = f.create();
    let added = f.call("backlog_add", json!({"path":"BACKLOG.md","section":"Open","title":"Long topic","body":"private unfinished idea","if_file_sha256":empty["sha256"]})).expect("valid test fixture");
    let id = added["entry_id"].clone();
    let promote_args = json!({"path":"BACKLOG.md","entry":{"id":id},"if_file_sha256":added["sha256"],"task":{"title":"One iteration","assignee":"beta","client_request_id":"iteration"}});
    let promoted = f
        .call("backlog_promote", promote_args.clone())
        .expect("valid test fixture");
    let again = f
        .call("backlog_promote", promote_args.clone())
        .expect("valid test fixture");
    assert_eq!(promoted["task_id"], again["task_id"]);
    assert_eq!(again["reused"], true);
    let task_id = promoted["task_id"].clone();
    let done = f
        .call(
            "context_sync",
            json!({"ops":[{"op":"task.move","task_id":task_id,"status":"done"}]}),
        )
        .expect("valid test fixture");
    assert_eq!(done["backlog_followup"][0]["source"]["entry_id"], id);
    let repeated = f
        .call(
            "context_sync",
            json!({"ops":[{"op":"task.move","task_id":task_id,"status":"done"}]}),
        )
        .expect("valid test fixture");
    assert!(!repeated.contains_key("backlog_followup"));
    assert_eq!(
        f.call("backlog_promote", promote_args)
            .expect("valid test fixture")["task_id"],
        task_id
    );
    let snapshot = f
        .call("backlog_get", json!({}))
        .expect("valid test fixture");
    assert_eq!(snapshot["sections"][1]["closed"], false);
    assert_eq!(
        snapshot["sections"][1]["entries"][0]["tasks"][0]["status"],
        "done"
    );
    let closed = f.call("backlog_close", json!({"path":"BACKLOG.md","entry":{"id":id},"outcome":"Resolved","if_file_sha256":snapshot["sha256"]})).expect("valid test fixture");
    assert_eq!(closed["sections"][2]["entries"][0]["id"], id);
    assert!(
        closed["sections"][2]["entries"][0]["body"]
            .as_str()
            .expect("valid test fixture")
            .contains("Resolved")
    );
    let ledger = std::fs::read_to_string(
        GroupStore::new(f.home.clone())
            .expect("valid test fixture")
            .ledger_path(&f.group.group_id)
            .expect("valid test fixture"),
    )
    .expect("valid test fixture");
    assert!(!ledger.contains("private unfinished idea"));
    f.call(
        "context_sync",
        json!({"ops":[{"op":"task.move","task_id":task_id,"status":"planned"}]}),
    )
    .expect("valid test fixture");
    let task = f
        .call("task_list", json!({"task_id":task_id}))
        .expect("valid test fixture");
    assert_eq!(task["delete_info"]["allowed"], false);
    assert!(
        f.call(
            "context_sync",
            json!({"ops":[{"op":"task.delete","task_id":task_id}]})
        )
        .is_err()
    );
}

#[test]
fn promote_uses_contexts_normalized_retry_key_for_source_checks_and_replay() {
    let f = Fixture::new();
    f.create();
    let first = f
        .call(
            "backlog_add",
            json!({"path":"BACKLOG.md","section":"Open","title":"One","body":""}),
        )
        .expect("first entry");
    let promoted = f
        .call("backlog_promote", json!({"path":"BACKLOG.md","entry":{"id":first["entry_id"]},"if_file_sha256":first["sha256"],"task":{"title":"Work","client_request_id":"iteration"}}))
        .expect("first task");
    f.call(
        "context_sync",
        json!({"ops":[{"op":"task.move","task_id":promoted["task_id"],"status":"done"}]}),
    )
    .expect("finish task");
    let replay = f
        .call("backlog_promote", json!({"path":"BACKLOG.md","entry":{"id":first["entry_id"]},"if_file_sha256":first["sha256"],"task":{"title":"Work","client_request_id":" iteration "}}))
        .expect("same normalized key replays completed task");
    assert_eq!(replay["task_id"], promoted["task_id"]);
    assert_eq!(replay["reused"], true);

    let second = f
        .call(
            "backlog_add",
            json!({"path":"BACKLOG.md","section":"Open","title":"Two","body":""}),
        )
        .expect("second entry");
    let before = std::fs::read(f.root.join("BACKLOG.md")).expect("file before rejection");
    let error = f
        .call("backlog_promote", json!({"path":"BACKLOG.md","entry":{"id":second["entry_id"]},"if_file_sha256":second["sha256"],"task":{"title":"Other work","client_request_id":" iteration "}}))
        .expect_err("retry key already belongs to another source");
    assert_eq!(error.code, "invalid_args");
    assert_eq!(
        std::fs::read(f.root.join("BACKLOG.md")).expect("file after rejection"),
        before
    );
}

#[test]
fn stale_hash_scope_file_switch_and_peer_permissions_refuse_before_writing() {
    let f = Fixture::new();
    let empty = f.create();
    assert_eq!(
        f.call("backlog_exclude", json!({"by":"beta","path":"BACKLOG.md"}))
            .expect_err("operation is rejected")
            .code,
        "permission_denied"
    );
    let original = std::fs::read_to_string(f.root.join("BACKLOG.md")).expect("valid test fixture");
    std::fs::write(
        f.root.join("BACKLOG.md"),
        format!("{original}\nexternal edit\n"),
    )
    .expect("valid test fixture");
    assert_eq!(f.call("backlog_add", json!({"path":"BACKLOG.md","section":"Open","title":"new","body":"","if_file_sha256":empty["sha256"]})).expect_err("operation is rejected").code, "backlog_file_conflict");
    assert_eq!(f.call("backlog_add", json!({"path":"BACKLOG.md","section":"Open","title":"new","body":"","scope_url":"/different"})).expect_err("operation is rejected").code, "scope_conflict");
    std::fs::write(f.root.join("other.md"), original).expect("valid test fixture");
    f.call("backlog_use", json!({"path":"other.md"}))
        .expect("valid test fixture");
    assert_eq!(
        f.call(
            "backlog_add",
            json!({"path":"BACKLOG.md","section":"Open","title":"new","body":""})
        )
        .expect_err("operation is rejected")
        .code,
        "backlog_file_conflict"
    );
    assert_eq!(
        f.call("backlog_use", json!({"path":"../escape.md"}))
            .expect_err("operation is rejected")
            .code,
        "permission_denied"
    );
    assert_eq!(
        f.call("backlog_create", json!({"path":"missing/BACKLOG.md"}))
            .expect_err("operation is rejected")
            .code,
        "backlog_not_found"
    );
}

#[test]
fn idless_promote_validates_task_before_writing_and_reports_known_partial_file_success() {
    let f = Fixture::new();
    std::fs::write(f.root.join("BACKLOG.md"), "# Plan\n### Topic\nPrivate\n")
        .expect("valid test fixture");
    let snapshot = f
        .call("backlog_use", json!({"path":"BACKLOG.md"}))
        .expect("valid test fixture");
    let args = json!({"path":"BACKLOG.md","entry":{"line":2},"if_file_sha256":snapshot["sha256"],"task":{"title":"Work","assignee":"alpha"},"by":"beta"});
    assert_eq!(
        f.call("backlog_promote", args)
            .expect_err("operation is rejected")
            .code,
        "permission_denied"
    );
    assert!(
        !std::fs::read_to_string(f.root.join("BACKLOG.md"))
            .expect("valid test fixture")
            .contains("<!-- id:")
    );
    let ledger = GroupStore::new(f.home.clone())
        .expect("valid test fixture")
        .ledger_path(&f.group.group_id)
        .expect("valid test fixture");
    std::fs::remove_file(&ledger).expect("valid test fixture");
    std::fs::create_dir(&ledger).expect("valid test fixture");
    let error = f.call("backlog_promote", json!({"path":"BACKLOG.md","entry":{"line":2},"if_file_sha256":snapshot["sha256"],"task":{"title":"Work"}})).expect_err("operation is rejected");
    assert_eq!(error.code, "backlog_partial_failure");
    assert!(
        error.details["applied"]["file"]["entry_id"]
            .as_str()
            .is_some()
    );
    assert!(
        std::fs::read_to_string(f.root.join("BACKLOG.md"))
            .expect("valid test fixture")
            .contains("<!-- id:")
    );
}

#[test]
fn git_local_exclusion_escapes_literal_paths_and_never_untracks() {
    let f = Fixture::new();
    let init = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&f.root)
        .status()
        .expect("valid test fixture");
    assert!(init.success());
    let result = f
        .call(
            "backlog_create",
            json!({"path":"plan [1]*.md","keep_local":true}),
        )
        .expect("valid test fixture");
    assert_eq!(result["git"]["ignored"], true);
    assert_eq!(result["git"]["tracked"], false);
    std::fs::write(f.root.join("tracked.md"), "# Plan\n").expect("valid test fixture");
    assert!(
        std::process::Command::new("git")
            .current_dir(&f.root)
            .args(["add", "--", "tracked.md"])
            .status()
            .expect("valid test fixture")
            .success()
    );
    f.call("backlog_use", json!({"path":"tracked.md"}))
        .expect("valid test fixture");
    assert_eq!(
        f.call("backlog_exclude", json!({"path":"tracked.md"}))
            .expect_err("operation is rejected")
            .code,
        "backlog_tracked"
    );
}
