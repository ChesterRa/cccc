use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value};

pub(super) fn write_skill(
    root: &std::path::Path,
    name: &str,
    description: &str,
    body: &str,
) -> std::path::PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory).expect("skill directory");
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n"),
    )
    .expect("skill");
    directory
}

pub(super) fn capability_events(home: &HomeLayout, group_id: &str) -> Vec<cccc_contracts::Event> {
    let path = GroupStore::new(home.clone())
        .expect("groups")
        .ledger_path(group_id)
        .expect("ledger path");
    ledger::read_all(&path)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| event.kind == "capability.changed")
        .collect()
}

pub(super) fn call(home: &HomeLayout, op: &str, args: Value) -> Map<String, Value> {
    let response = response(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response.result
}

pub(super) fn response(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        },
    )
}
