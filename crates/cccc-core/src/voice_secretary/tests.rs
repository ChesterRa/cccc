use super::*;
use cccc_contracts::voice_secretary::SecretaryTaskKind;
use serde_json::json;

fn fixture() -> (
    tempfile::TempDir,
    SecretaryTaskStore,
    SecretaryTask,
    PathBuf,
) {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let store = SecretaryTaskStore::new(home);
    let mut task = SecretaryTask::new(
        SecretaryTaskTarget {
            group_id: "group-a".into(),
            scope_key: "scope-a".into(),
            kind: SecretaryTaskKind::Document,
            document_id: "doc-a".into(),
            document_path: "notes.md".into(),
            request_id: String::new(),
            composer_snapshot_hash: String::new(),
        },
        vec![json!({"input_id":"input-a","seq":7,"text":"Alice said 42."})],
    );
    task.phase = SecretaryTaskPhase::Starting;
    store.create(&task).expect("create");
    let file = temp.path().join("notes.md");
    std::fs::write(&file, b"# Original\n").expect("document");
    store
        .prepare_document(&task.task_id, &file)
        .expect("snapshot");
    store
        .update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Running;
            Ok(())
        })
        .expect("admission");
    task = store.load(&task.task_id).expect("task");
    (temp, store, task, file)
}

#[test]
fn commit_fixes_target_and_source_range_without_claiming_process_cleanup() {
    let (_temp, store, task, file) = fixture();
    std::fs::write(
        store
            .workspace(&task.task_id)
            .expect("workspace")
            .join("document.md"),
        b"# Original\nAlice said 42.\n",
    )
    .expect("candidate");
    let receipt = store
        .commit_document(&task.task_id, &task.base_version, |t| {
            assert_eq!(t.target.group_id, "group-a");
            assert_eq!(t.target.scope_key, "scope-a");
            assert_eq!(t.inputs[0]["seq"], 7);
            Ok(())
        })
        .expect("commit");
    assert_eq!(receipt.status, SecretaryTaskPhase::Done);
    assert_eq!(
        std::fs::read_to_string(file).expect("original"),
        "# Original\nAlice said 42.\n"
    );
    let saved = store.load(&task.task_id).expect("saved");
    assert_eq!(saved.phase, SecretaryTaskPhase::Running);
    assert!(saved.receipt.is_some());
    // A changed candidate cannot cause a second write through an idempotent receipt.
    std::fs::write(
        store
            .workspace(&task.task_id)
            .expect("workspace")
            .join("document.md"),
        "other",
    )
    .expect("later candidate");
    assert_eq!(
        store
            .commit_document(&task.task_id, &task.base_version, |_| Ok(()))
            .expect("repeated"),
        receipt
    );
}

#[test]
fn manual_edit_conflicts_and_retains_candidate_and_unprocessed_sources() {
    let (_temp, store, task, file) = fixture();
    let candidate = store
        .workspace(&task.task_id)
        .expect("workspace")
        .join("document.md");
    std::fs::write(&candidate, "candidate").expect("candidate");
    std::fs::write(&file, "user changed it").expect("manual edit");
    let receipt = store
        .commit_document(&task.task_id, &task.base_version, |_| Ok(()))
        .expect("conflict");
    assert_eq!(receipt.status, SecretaryTaskPhase::Conflict);
    assert_eq!(
        std::fs::read_to_string(file).expect("original"),
        "user changed it"
    );
    assert_eq!(
        std::fs::read_to_string(candidate).expect("candidate retained"),
        "candidate"
    );
    assert_eq!(
        store.load(&task.task_id).expect("task").inputs[0]["input_id"],
        "input-a"
    );
}

#[test]
fn recovery_finishes_a_submitted_commit_without_replaying_a_prompt() {
    for already_written in [false, true] {
        let (_temp, store, task, file) = fixture();
        let submitted = b"submitted artifact";
        std::fs::write(
            store
                .directory(&task.task_id)
                .expect("dir")
                .join("prepared.md"),
            submitted,
        )
        .expect("submission");
        store
            .update(&task.task_id, |t| {
                t.prepared_version = digest(submitted);
                Ok(())
            })
            .expect("prepared receipt");
        if already_written {
            std::fs::write(&file, submitted).expect("write before crash");
        }
        store.recover(&task.task_id, |_| Ok(())).expect("recover");
        let saved = store.load(&task.task_id).expect("task");
        assert_eq!(saved.phase, SecretaryTaskPhase::Done);
        assert_eq!(
            saved.receipt.expect("business receipt").document_version,
            digest(submitted)
        );
        assert_eq!(std::fs::read(file).expect("original"), submitted);
    }
}

#[test]
fn an_unconfirmed_provider_is_never_returned_to_the_queue() {
    let (_temp, store, task, _) = fixture();
    store.recover(&task.task_id, |_| Ok(())).expect("recover");
    assert_eq!(
        store.load(&task.task_id).expect("task").phase,
        SecretaryTaskPhase::Unconfirmed
    );
}

#[test]
fn cancellation_and_target_retirement_fence_late_commits() {
    let (_temp, store, task, file) = fixture();
    assert!(
        store
            .commit_document(&task.task_id, &task.base_version, |_| Err(
                io::Error::other("document archived")
            ))
            .is_err()
    );
    store
        .update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Cancelled;
            Ok(())
        })
        .expect("cancel");
    assert!(
        store
            .commit_document(&task.task_id, &task.base_version, |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(file).expect("original"),
        "# Original\n"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_candidate_cannot_bypass_controlled_submission() {
    let (_temp, store, task, file) = fixture();
    let candidate = store
        .workspace(&task.task_id)
        .expect("workspace")
        .join("document.md");
    std::fs::remove_file(&candidate).expect("remove candidate");
    std::os::unix::fs::symlink(&file, candidate).expect("symlink");
    assert!(
        store
            .commit_document(&task.task_id, &task.base_version, |_| Ok(()))
            .is_err()
    );
}

#[test]
fn a_done_receipt_stays_in_unfinished_views_until_cleanup_is_confirmed() {
    let (_temp, disk, task, _file) = fixture();
    let indexed = SecretaryTaskStore::indexed(disk.home.clone()).expect("index");
    indexed
        .update(&task.task_id, |task| {
            task.phase = SecretaryTaskPhase::Done;
            task.cleanup_confirmed = false;
            Ok(())
        })
        .expect("business done, process pending");
    assert_eq!(disk.unfinished().expect("disk").len(), 1);
    assert_eq!(indexed.unfinished().expect("index").len(), 1);
    indexed
        .update(&task.task_id, |task| {
            task.cleanup_confirmed = true;
            Ok(())
        })
        .expect("cleanup done");
    assert!(disk.unfinished().expect("disk").is_empty());
    assert!(indexed.unfinished().expect("index").is_empty());
}

#[test]
fn shared_index_publishes_writes_and_reloads_durable_receipts() {
    let (_temp, disk, task, _file) = fixture();
    let indexed = SecretaryTaskStore::indexed(disk.home.clone()).expect("load once");
    let clone = indexed.clone();
    indexed
        .update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Cancelled;
            Ok(())
        })
        .expect("persist");
    assert_eq!(
        clone.load(&task.task_id).expect("shared").phase,
        SecretaryTaskPhase::Cancelled
    );
    assert_eq!(
        clone.source_task("group-a", "input-a").expect("source"),
        Some(task.task_id.clone())
    );
    assert!(clone.queued().expect("queue").is_empty());
    let mut next = SecretaryTask::new(task.target.clone(), task.inputs.clone());
    next.previous_task_id = task.task_id.clone();
    clone.create(&next).expect("successor");
    assert_eq!(
        indexed
            .successor(&task.task_id)
            .expect("successor")
            .expect("next")
            .task_id,
        next.task_id
    );
    let restarted = SecretaryTaskStore::indexed(disk.home.clone()).expect("restart");
    assert_eq!(restarted.queued().expect("queue").len(), 1);
    let path = disk
        .directory(&task.task_id)
        .expect("directory")
        .join("task.json");
    std::fs::rename(&path, path.with_extension("temporarily-unavailable"))
        .expect("offline read probe");
    assert_eq!(
        clone.load(&task.task_id).expect("reads use index").phase,
        SecretaryTaskPhase::Cancelled
    );
    assert_eq!(clone.group_tasks("group-a").expect("projection").len(), 2);
}

#[test]
fn unprojected_ask_outcomes_remain_visible_outside_recent_history() {
    let (_temp, store, original, _file) = fixture();
    for phase in [SecretaryTaskPhase::Done, SecretaryTaskPhase::Cancelled] {
        let mut target = original.target.clone();
        target.kind = SecretaryTaskKind::Ask;
        let mut task = SecretaryTask::new(target, original.inputs.clone());
        task.phase = phase;
        task.cleanup_confirmed = true;
        task.created_at = "2020-01-01T00:00:00Z".into();
        store.create(&task).expect("unprojected outcome");
        let view = store.group_view(&task.target.group_id, 0).expect("view");
        assert!(view.tasks.iter().any(|item| item.task_id == task.task_id));
        assert!(view.pending, "projection is still outstanding");
        store
            .update(&task.task_id, |saved| {
                saved.projected_at = utc_now();
                Ok(())
            })
            .expect("projected outcome");
        assert!(
            !store
                .group_view(&task.target.group_id, 0)
                .expect("bounded settled history")
                .tasks
                .iter()
                .any(|item| item.task_id == task.task_id)
        );
    }
}

#[test]
fn completed_working_copies_retire_only_after_projection_and_cleanup() {
    let (_temp, store, task, _file) = fixture();
    store
        .commit_document(&task.task_id, &task.base_version, |_| Ok(()))
        .expect("commit");
    store.retire_artifacts(&task.task_id).expect("not yet");
    assert!(store.workspace(&task.task_id).expect("workspace").exists());
    store
        .update(&task.task_id, |t| {
            t.phase = SecretaryTaskPhase::Done;
            t.cleanup_confirmed = false;
            t.projected_at = utc_now();
            Ok(())
        })
        .expect("projection");
    store
        .retire_artifacts(&task.task_id)
        .expect("cleanup pending");
    assert!(store.workspace(&task.task_id).expect("workspace").exists());
    store
        .update(&task.task_id, |t| {
            t.cleanup_confirmed = true;
            Ok(())
        })
        .expect("cleanup");
    store
        .update(&task.task_id, |t| {
            t.inputs[0]["metadata"] = json!({"user_preview":"本日の会議", "other":"discard"});
            Ok(())
        })
        .expect("display summary");
    store.retire_artifacts(&task.task_id).expect("retire");
    let saved = store.load(&task.task_id).expect("compact receipt");
    assert!(saved.artifacts_retired);
    assert_eq!(saved.preview(), "本日の会議");
    assert!(saved.inputs[0]["metadata"]["other"].is_null());
    assert!(saved.covers(&task.inputs[0]));
    assert!(saved.receipt.is_some());
    assert!(!store.workspace(&task.task_id).expect("workspace").exists());
    assert!(
        !store
            .directory(&task.task_id)
            .expect("directory")
            .join("prepared.md")
            .exists()
    );
}

#[test]
fn successful_continuation_retires_obsolete_candidate_without_erasing_failure_receipt() {
    let (_temp, store, old, file) = fixture();
    std::fs::write(&file, "manual edit").expect("edit");
    store
        .commit_document(&old.task_id, &old.base_version, |_| Ok(()))
        .expect("conflict");
    let mut next = SecretaryTask::new(old.target.clone(), old.inputs.clone());
    next.previous_task_id = old.task_id.clone();
    next.phase = SecretaryTaskPhase::Starting;
    store.create(&next).expect("continuation");
    store
        .prepare_document(&next.task_id, &file)
        .expect("fresh snapshot");
    store
        .update(&old.task_id, |t| {
            t.phase = SecretaryTaskPhase::Conflict;
            t.cleanup_confirmed = true;
            t.superseded_by = next.task_id.clone();
            Ok(())
        })
        .expect("link");
    store
        .retire_resolved_artifacts(&old.task_id, &next.task_id)
        .expect("not resolved");
    assert!(store.workspace(&old.task_id).expect("candidate").exists());
    let base = store.load(&next.task_id).expect("snapshot").base_version;
    store
        .commit_document(&next.task_id, &base, |_| Ok(()))
        .expect("confirmed submit");
    store
        .update(&next.task_id, |t| {
            t.phase = SecretaryTaskPhase::Done;
            t.cleanup_confirmed = true;
            t.projected_at = utc_now();
            Ok(())
        })
        .expect("settled");
    store
        .retire_artifacts(&next.task_id)
        .expect("retire new copy");
    store
        .retire_resolved_artifacts(&old.task_id, &next.task_id)
        .expect("retire obsolete copy");
    let old = store.load(&old.task_id).expect("original receipt");
    assert!(old.artifacts_retired);
    assert_eq!(old.phase, SecretaryTaskPhase::Conflict);
    assert_eq!(
        old.receipt.expect("receipt").status,
        SecretaryTaskPhase::Conflict
    );
    assert!(!store.workspace(&old.task_id).expect("candidate").exists());
}

#[test]
fn indexed_group_views_bound_history_and_keep_gaps_and_cleanup_visible() {
    let (_temp, disk, original, _file) = fixture();
    for number in 0..48 {
        let mut task = SecretaryTask::new(
            original.target.clone(),
            vec![json!({"input_id":format!("history-{number}"),"text":"old"})],
        );
        task.created_at = format!("2025-01-01T00:00:{number:02}Z");
        task.phase = SecretaryTaskPhase::Done;
        task.projected_at = utc_now();
        task.guidance = "old instructions".repeat(1000);
        disk.create(&task).expect("history");
    }
    let store = SecretaryTaskStore::indexed(disk.home.clone()).expect("index");
    let view = store.group_view("group-a", 12).expect("view");
    assert_eq!(view.tasks.len(), 13);
    assert!(view.busy);
    assert_eq!(view.unprocessed_document_sources, 1);
    store
        .update(&original.task_id, |task| {
            task.phase = SecretaryTaskPhase::Cancelled;
            Ok(())
        })
        .expect("cancel");
    let view = store.group_view("group-a", 12).expect("view");
    assert!(view.pending);
    assert!(!view.busy);
    assert_eq!(view.unprocessed_document_sources, 1);
    store
        .update(&original.task_id, |task| {
            task.phase = SecretaryTaskPhase::Done;
            task.cleanup_confirmed = false;
            Ok(())
        })
        .expect("await cleanup");
    let view = store.group_view("group-a", 12).expect("view");
    assert!(view.busy && view.pending);
    assert_eq!(view.unprocessed_document_sources, 0);
    assert!(
        view.tasks
            .iter()
            .any(|task| task.task_id == original.task_id)
    );
    store
        .update(&original.task_id, |task| {
            task.cleanup_confirmed = true;
            task.projected_at = utc_now();
            Ok(())
        })
        .expect("cleanup");
    let view = store.group_view("group-a", 12).expect("settled");
    assert_eq!(view.tasks.len(), 12);
    assert!(!view.busy && !view.pending);
    assert!(
        store
            .source_task("group-a", "input-a")
            .expect("source retained")
            .is_some()
    );
    assert!(
        store
            .group_view("other-group", 12)
            .expect("isolated")
            .tasks
            .is_empty()
    );
    store.remove(&original.task_id).expect("remove");
    assert_eq!(
        store
            .group_view("group-a", 12)
            .expect("after removal")
            .unprocessed_document_sources,
        0
    );
}

#[test]
fn legacy_task_grants_are_revoked_without_removing_the_working_copy() {
    let (_temp, store, task, _file) = fixture();
    let grant = store.grant_file(&task.task_id).expect("legacy grant path");
    std::fs::write(&grant, "synthetic revoked grant").expect("legacy fixture");
    store.revoke_grant_file(&task.task_id).expect("revoke");
    assert!(!grant.exists());
    store
        .revoke_grant_file(&task.task_id)
        .expect("already revoked");
    assert!(
        store
            .workspace(&task.task_id)
            .expect("isolated fixture invariant")
            .exists()
    );
}

#[test]
fn empty_display_metadata_keeps_plain_document_sources_but_never_prompt_envelopes() {
    let (_temp, _store, mut task, _file) = fixture();
    task.inputs[0]["text"] = json!("会议预算调整");
    task.inputs[0]["metadata"] = json!({"user_preview":""});
    assert_eq!(task.preview(), "会议预算调整");
    task.target.kind = cccc_contracts::voice_secretary::SecretaryTaskKind::Prompt;
    task.inputs[0]["text"] = json!("Target: composer Request kind: prompt_refine");
    assert!(task.preview().is_empty());
}

#[test]
fn relocating_a_retained_working_copy_preserves_candidate_and_receipt() {
    let (_temp, store, task, _file) = fixture();
    let current = store
        .workspace(&task.task_id)
        .expect("isolated fixture invariant");
    let old = store
        .directory(&task.task_id)
        .expect("isolated fixture invariant")
        .join("workspace");
    std::fs::write(current.join("document.md"), "retained candidate")
        .expect("isolated fixture invariant");
    std::fs::rename(&current, &old).expect("isolated fixture invariant");
    store
        .relocate_workspace(&task.task_id)
        .expect("isolated fixture invariant");
    assert_eq!(
        store
            .candidate(&task.task_id)
            .expect("isolated fixture invariant"),
        "retained candidate"
    );
    assert_eq!(
        store
            .load(&task.task_id)
            .expect("isolated fixture invariant")
            .base_version,
        task.base_version
    );
    assert!(!old.exists());
    store
        .relocate_workspace(&task.task_id)
        .expect("isolated fixture invariant");
    std::fs::create_dir(&old).expect("isolated fixture invariant");
    std::fs::write(old.join("document.md"), "conflicting old copy")
        .expect("isolated fixture invariant");
    assert!(store.relocate_workspace(&task.task_id).is_err());
    assert_eq!(
        store
            .candidate(&task.task_id)
            .expect("isolated fixture invariant"),
        "retained candidate"
    );
    assert_eq!(
        std::fs::read_to_string(old.join("document.md")).expect("isolated fixture invariant"),
        "conflicting old copy"
    );
}

#[test]
fn malformed_task_records_identify_the_task_without_exposing_parser_values() {
    let (_temp, store, task, _file) = fixture();
    let path = store
        .directory(&task.task_id)
        .expect("directory")
        .join("task.json");
    let mut value = serde_json::to_value(&task).expect("task");
    value["phase"] = json!("synthetic-private-phase-must-not-be-exposed");
    std::fs::write(&path, serde_json::to_vec(&value).expect("fixture")).expect("corrupt task");
    let before = std::fs::read(&path).expect("fixture bytes");
    for error in [
        store.load(&task.task_id).expect_err("invalid task"),
        SecretaryTaskStore::indexed(store.home.clone())
            .err()
            .expect("index fails closed"),
    ] {
        let diagnostic = error.to_string();
        assert!(diagnostic.contains(&task.task_id), "{diagnostic}");
        assert!(diagnostic.contains("JSON"), "{diagnostic}");
        assert!(!diagnostic.contains("synthetic-private"), "{diagnostic}");
    }
    assert_eq!(std::fs::read(&path).expect("retained bytes"), before);
}
