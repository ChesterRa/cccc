use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, Registry};
use serde_json::{Map, Value, json};

#[test]
fn registry_reconcile_reports_health_and_removes_only_missing_entries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let missing = store.create("missing", "").expect("missing group");
    let corrupt = store.create("corrupt", "").expect("corrupt group");
    Registry::mutate(&home, |registry| {
        registry
            .defaults
            .insert("scope-missing".into(), missing.group_id.clone());
        registry
            .defaults
            .insert("scope-corrupt".into(), corrupt.group_id.clone());
        Ok(())
    })
    .expect("defaults");
    std::fs::remove_file(
        store
            .group_dir(&missing.group_id)
            .expect("missing dir")
            .join("group.yaml"),
    )
    .expect("remove group document");
    std::fs::write(
        store
            .group_dir(&corrupt.group_id)
            .expect("corrupt dir")
            .join("group.yaml"),
        "not: [valid",
    )
    .expect("corrupt group document");

    let preview = ok(
        &home,
        "registry_reconcile",
        json!({"remove_missing":false,"by":"user"}),
    );
    assert_eq!(preview.result["dry_run"], true);
    assert_eq!(preview.result["scanned_groups"], 2);
    assert_eq!(
        preview.result["missing_group_ids"],
        json!([missing.group_id])
    );
    assert_eq!(
        preview.result["corrupt_group_ids"],
        json!([corrupt.group_id])
    );
    assert_eq!(preview.result["removed_group_ids"], json!([]));

    let cleaned = ok(
        &home,
        "registry_reconcile",
        json!({"remove_missing":true,"by":"user"}),
    );
    assert_eq!(
        cleaned.result["removed_group_ids"],
        json!([missing.group_id])
    );
    assert_eq!(
        cleaned.result["removed_default_scope_keys"],
        json!(["scope-missing"])
    );
    let registry = Registry::load(&home).expect("registry");
    assert!(!registry.groups.contains_key(&missing.group_id));
    assert!(registry.groups.contains_key(&corrupt.group_id));
    assert!(!registry.defaults.contains_key("scope-missing"));
    assert_eq!(registry.defaults["scope-corrupt"], corrupt.group_id);
}

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{:?}", response.error);
    response
}
