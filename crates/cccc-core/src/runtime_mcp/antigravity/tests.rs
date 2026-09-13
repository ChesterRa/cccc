use super::*;
use serde_json::json;

fn environment(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), root.display().to_string()),
        ("USERPROFILE".into(), root.display().to_string()),
    ])
}

fn write(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directories");
    std::fs::write(path, serde_json::to_vec(value).expect("json")).expect("config");
}

fn ready() -> Value {
    json!({"command":"cccc","args":["mcp"],"disabled":false})
}

#[test]
fn native_setup_repairs_once_and_keeps_instance_identity_inherited() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("project");
    let path = temp.path().join(".gemini/config/mcp_config.json");
    let other = json!({"command":"other","args":["keep"]});
    write(
        &path,
        &json!({"mcpServers":{"other":other,"cccc":{
        "command":"/old/cccc","args":["mcp"],"disabled":true,
        "env":{"CCCC_HOME":"/old/instance"}}}}),
    );
    let mut env = environment(temp.path());
    env.insert("CCCC_HOME".into(), "/instance/a".into());
    ensure(&cwd, &env, || {
        let mut doc: Value = serde_json::from_slice(&std::fs::read(&path)?).expect("config");
        doc["mcpServers"]["cccc"] = ready();
        write(&path, &doc);
        Ok(())
    })
    .expect("native repair");
    env.insert("CCCC_HOME".into(), "/instance/b".into());
    env.insert("CCCC_ACTOR_ID".into(), "other-actor".into());
    ensure(&cwd, &env, || {
        panic!("shared entry must not be rewritten per instance")
    })
    .expect("second instance inherits its own environment");
    let doc: Value = serde_json::from_slice(&std::fs::read(path).expect("read")).expect("config");
    assert_eq!(doc["mcpServers"]["other"], other);
    assert!(doc["mcpServers"]["cccc"].get("env").is_none());
}

#[test]
fn project_entry_takes_precedence_and_conflicts_are_not_overwritten() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("project");
    let path = cwd.join(".agents/mcp_config.json");
    let doc = json!({"mcpServers":{"cccc":{"command":"wrong","args":["mcp"]}}});
    write(&path, &doc);
    write(
        &temp.path().join(".gemini/config/mcp_config.json"),
        &json!({"mcpServers":{"cccc":ready()}}),
    );
    let error = ensure(&cwd, &environment(temp.path()), || {
        panic!("must not modify global config")
    })
    .expect_err("project conflict");
    assert!(error.to_string().contains("overrides the global"));
    assert_eq!(
        std::fs::read(&path).expect("read"),
        serde_json::to_vec(&doc).expect("json")
    );
    write(&path, &json!({"mcpServers":{"cccc":ready()}}));
    assert_eq!(
        ensure(&cwd, &environment(temp.path()), || panic!("already ready")).expect("project entry"),
        path
    );
}

#[test]
fn malformed_configuration_is_preserved_before_invoking_native_cli() {
    for relative in [
        ".gemini/config/mcp_config.json",
        "project/.agents/mcp_config.json",
    ] {
        for bytes in ["{broken", "[]", r#"{"mcpServers":null}"#] {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(&path, bytes).expect("malformed config");
            let error = ensure(
                &temp.path().join("project"),
                &environment(temp.path()),
                || panic!("no mutation"),
            )
            .expect_err("invalid config");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(std::fs::read_to_string(path).expect("read"), bytes);
        }
    }
}

#[test]
fn setup_failure_or_unverified_success_cannot_report_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let env = environment(temp.path());
    let cwd = temp.path().join("project");
    let error =
        ensure(&cwd, &env, || Err(io::Error::other("native setup failed"))).expect_err("failure");
    assert!(error.to_string().contains("native setup failed"));
    let error = ensure(&cwd, &env, || Ok(())).expect_err("exit zero is not config verification");
    assert!(error.to_string().contains("did not produce"));
}

#[test]
fn ready_configuration_does_not_require_a_writable_provider_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join(".gemini/config/mcp_config.json");
    write(&path, &json!({"mcpServers":{"cccc":ready()}}));
    let settings = temp.path().join(".gemini/antigravity-cli/settings.json");
    write(&settings, &json!({"showFeedbackSurvey":false}));
    ensure(
        &temp.path().join("project"),
        &environment(temp.path()),
        || panic!("no setup"),
    )
    .expect("read-only startup");
    assert!(!path.with_extension("cccc.lock").exists());
    assert!(!settings.with_extension("cccc.lock").exists());
}

#[test]
fn setup_disables_input_consuming_surveys_and_preserves_other_preferences() {
    let temp = tempfile::tempdir().expect("tempdir");
    let settings = temp.path().join(".gemini/antigravity-cli/settings.json");
    let expected = json!({"showFeedbackSurvey":false,"model":"chosen-model","enableTelemetry":true,"custom":{"keep":[1,2]}});
    for initial in [
        None,
        Some(
            json!({"showFeedbackSurvey":true,"model":"chosen-model","enableTelemetry":true,"custom":{"keep":[1,2]}}),
        ),
    ] {
        if let Some(initial) = initial {
            write(&settings, &initial);
        }
        disable_feedback_survey(&settings).expect("prepare input");
        let actual = read_settings(&settings).expect("settings");
        assert_eq!(actual["showFeedbackSurvey"], false);
        if actual.contains_key("model") {
            assert_eq!(Value::Object(actual), expected);
        }
        let before = std::fs::read(&settings).expect("read");
        disable_feedback_survey(&settings).expect("already prepared");
        assert_eq!(std::fs::read(&settings).expect("read"), before);
    }
}

#[test]
fn invalid_native_preferences_are_not_replaced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("settings.json");
    for bytes in ["{broken", "[]", "null"] {
        std::fs::write(&path, bytes).expect("write");
        assert_eq!(
            disable_feedback_survey(&path)
                .expect_err("invalid preferences")
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), bytes);
    }
}

#[cfg(unix)]
#[test]
fn native_preference_symlink_is_preserved() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("shared.json");
    let path = temp.path().join("settings.json");
    write(&target, &json!({"showFeedbackSurvey":true,"model":"keep"}));
    std::os::unix::fs::symlink(&target, &path).expect("symlink");
    disable_feedback_survey(&path).expect("update target");
    assert!(path.is_symlink());
    assert_eq!(read_settings(&target).expect("settings")["model"], "keep");
    assert_eq!(
        read_settings(&target).expect("settings")["showFeedbackSurvey"],
        false
    );
    let broken = temp.path().join("broken.json");
    std::os::unix::fs::symlink(temp.path().join("missing.json"), &broken).expect("broken link");
    assert!(disable_feedback_survey(&broken).is_err());
    assert!(
        broken.is_symlink(),
        "do not replace user-owned broken links"
    );
}

#[test]
fn effective_entry_must_keep_bootstrap_available_and_inherit_actor_context() {
    for patch in [
        json!({"disabled":true}),
        json!({"disabledTools":["cccc_bootstrap"]}),
        json!({"env":{"CCCC_ACTOR_ID":"old"}}),
        json!({"env":{"PATH":"/wrong"}}),
        json!({"cwd":"/wrong"}),
        json!({"serverUrl":"https://example.invalid"}),
    ] {
        let mut entry = ready();
        entry
            .as_object_mut()
            .expect("object")
            .extend(patch.as_object().expect("patch").clone());
        assert!(!matches(&entry));
    }
    let mut entry = ready();
    entry["env"] = json!({"UNRELATED":"keep"});
    assert!(matches(&entry));
}

#[test]
fn concurrent_instances_do_not_race_native_registration() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let calls = AtomicUsize::new(0);
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let barrier = barrier.clone();
            let root = temp.path();
            let calls = &calls;
            scope.spawn(move || {
                barrier.wait();
                ensure(&root.join("project"), &environment(root), || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    write(
                        &root.join(".gemini/config/mcp_config.json"),
                        &json!({"mcpServers":{"cccc":ready()}}),
                    );
                    Ok(())
                })
                .expect("concurrent setup");
            });
        }
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
