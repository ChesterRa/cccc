use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn python_and_rust_share_context_tasks_and_version_state() {
    let repo = workspace_root();
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("interop", "").expect("group");
    let contexts = ContextStore::new(home).expect("contexts");
    let create = json!({"op":"task.create","title":"created by Rust"})
        .as_object()
        .cloned()
        .expect("operation");
    let agent = json!({
        "op":"agent_state.update",
        "actor_id":"peer",
        "active_task_id":"T001",
        "focus":"Rust focus",
        "what_changed":"Rust update",
    })
    .as_object()
    .cloned()
    .expect("operation");
    let note = json!({
        "op":"coordination.note.add",
        "kind":"decision",
        "summary":"Rust decision",
    })
    .as_object()
    .cloned()
    .expect("operation");
    let rust_result = contexts
        .sync(&group.group_id, &[create, agent, note], None, "user", false)
        .expect("Rust sync");
    assert_eq!(rust_result.context.tasks[0]["id"], "T001");
    assert_eq!(rust_result.version, "ctxv:1");

    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.kernel.context import ContextStorage, Task
from cccc.kernel.group import load_group

group = load_group(sys.argv[1])
assert group is not None
storage = ContextStorage(group)
tasks = storage.list_tasks()
assert [(task.id, task.title) for task in tasks] == [("T001", "created by Rust")]
agents = storage.load_agents().agents
assert [(agent.id, agent.hot.focus) for agent in agents] == [("peer", "Rust focus")]
context = storage.load_context()
assert [note.summary for note in context.coordination.recent_decisions] == ["Rust decision"]
context.coordination.brief.objective = "updated by Python"
storage.save_context(context)
storage.save_task(Task(id=storage.generate_task_id(), title="created by Python"))
storage.update_agent_state("peer", "Python focus", active_task_id="T002")
storage.bump_version_state(context_changed=True, tasks_changed=True, agents_changed=True)
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("run Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let python_result = contexts.load(&group.group_id).expect("Rust reads Python");
    assert_eq!(python_result.revision, 2);
    assert_eq!(
        python_result.coordination["brief"]["objective"],
        "updated by Python"
    );
    assert_eq!(python_result.tasks.len(), 2);
    assert_eq!(python_result.tasks[1]["id"], "T002");
    assert_eq!(python_result.tasks[1]["title"], "created by Python");
    assert_eq!(
        python_result.agent_states["peer"]["hot"]["focus"],
        "Python focus"
    );

    let delete = json!({"op":"task.delete","task_id":"T002"})
        .as_object()
        .cloned()
        .expect("delete operation");
    contexts
        .sync(&group.group_id, &[delete], Some("ctxv:2"), "user", false)
        .expect("Rust delete");
    let output = python(&repo, temp.path())
        .arg(
            r#"
import sys
from cccc.kernel.context import ContextStorage
from cccc.kernel.group import load_group

group = load_group(sys.argv[1])
assert group is not None
assert [task.id for task in ContextStorage(group).list_tasks()] == ["T001"]
"#,
        )
        .arg(&group.group_id)
        .output()
        .expect("verify Python delete");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn legacy_rust_json_is_migrated_once_without_deleting_the_source() {
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("migration", "").expect("group");
    let group_dir = groups.group_dir(&group.group_id).expect("group dir");
    let mut legacy = ContextDoc::default();
    legacy.tasks.push(
        json!({"id":"t_legacy","title":"legacy Rust task","status":"done"})
            .as_object()
            .cloned()
            .expect("task"),
    );
    std::fs::write(
        group_dir.join("state/context.json"),
        serde_json::to_vec_pretty(&legacy).expect("legacy JSON"),
    )
    .expect("write legacy");

    let contexts = ContextStore::new(home).expect("contexts");
    let first = contexts.load(&group.group_id).expect("migrated context");
    let second = contexts.load(&group.group_id).expect("idempotent context");
    assert_eq!(first.tasks, second.tasks);
    assert_eq!(first.tasks.len(), 1);
    assert_eq!(first.tasks[0]["id"], "T001");
    assert!(group_dir.join("context/tasks/T001.yaml").is_file());
    assert!(
        group_dir
            .join("context/.rust-state-migrated-v1.json")
            .is_file()
    );
    assert!(group_dir.join("state/context.json").is_file());
}

fn python(repo: &Path, home: &Path) -> Command {
    let executable = std::env::var_os("CCCC_TEST_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo.join(if cfg!(windows) {
                ".venv/Scripts/python.exe"
            } else {
                ".venv/bin/python"
            })
        });
    let mut command = Command::new(executable);
    command
        .arg("-c")
        .env("CCCC_HOME", home)
        .env("PYTHONPATH", repo.join("src"))
        .current_dir(home);
    command
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
