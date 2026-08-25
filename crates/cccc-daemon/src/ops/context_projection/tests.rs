use super::*;

fn document() -> ContextDoc {
    let task = json!({
        "id":"task-1",
        "title":"Keep the board fast",
        "outcome":"Existing outcome",
        "status":"planned",
        "assignee":"worker",
        "priority":"high",
        "parent_id":"root-task",
        "updated_at":"2026-08-19T10:00:00Z",
        "blocked_by":["dependency"],
        "waiting_on":"actor",
        "handoff_to":"reviewer",
        "task_type":"standard",
        "notes":"large private notes",
        "checklist":[{"id":"check-1","text":"measure","status":"in_progress"}],
        "custom_detail":"full-only custom field"
    })
    .as_object()
    .cloned()
    .expect("task object");
    ContextDoc {
        tasks: vec![task],
        ..ContextDoc::default()
    }
}

#[test]
fn summary_preserves_every_editable_task_field() {
    let result = project(document(), "v1".into(), "summary");
    let task = &result["coordination"]["tasks"][0];

    assert_eq!(task["id"], "task-1");
    assert_eq!(task["assignee"], "worker");
    assert_eq!(task["priority"], "high");
    assert_eq!(task["parent_id"], "root-task");
    assert_eq!(task["waiting_on"], "actor");
    assert_eq!(task["handoff_to"], "reviewer");
    assert_eq!(task["task_type"], "standard");
    assert_eq!(task["notes"], "large private notes");
    assert_eq!(task["outcome"], "Existing outcome");
    assert_eq!(task["checklist"][0]["text"], "measure");
    assert!(task.get("custom_detail").is_none());
    assert!(result.get("board").is_none());
    assert_eq!(result["attention"]["blocked"], 1);
}

#[test]
fn full_context_keeps_task_objects_in_the_board_projection() {
    let result = project(document(), "v1".into(), "full");
    let task = &result["coordination"]["tasks"][0];

    assert_eq!(task["notes"], "large private notes");
    assert_eq!(task["checklist"][0]["text"], "measure");
    assert_eq!(result["board"]["planned"][0]["id"], "task-1");
    assert_eq!(result["board"]["planned"][0]["task_type"], "standard");
    assert_eq!(result["board"]["active"], json!([]));
    assert_eq!(result["tasks_summary"]["blocked"], 1);
    assert_eq!(result["attention"]["blocked"][0]["id"], "task-1");
}

#[test]
fn waiting_on_actor_or_external_is_blocked_without_a_blocked_by_list() {
    for waiting_on in ["actor", "external"] {
        let mut document = document();
        document.tasks[0].insert("blocked_by".into(), json!([]));
        document.tasks[0].insert("waiting_on".into(), json!(waiting_on));

        let summary = project(document, "v1".into(), "summary");

        assert_eq!(summary["attention"]["blocked"], 1, "{waiting_on}");
        assert_eq!(summary["tasks_summary"]["blocked"], 1, "{waiting_on}");
    }
}

#[test]
fn overview_omits_task_collections_but_keeps_coordination_notes() {
    let mut document = document();
    document.tasks_revision = 7;
    document
        .coordination
        .insert("recent_decisions".into(), json!([{"summary":"keep this"}]));

    let result = project(document, "v2".into(), "overview");

    assert_eq!(result["version"], "v2");
    assert_eq!(result["tasks_version"], "tasksv:7");
    assert_eq!(
        result["coordination"]["recent_decisions"][0]["summary"],
        "keep this"
    );
    assert!(result["coordination"].get("tasks").is_none());
    assert!(result.get("board").is_none());
    assert!(result.get("tasks_summary").is_none());
    assert!(result.get("attention").is_none());
}
