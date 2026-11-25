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
fn empty_configuration_from_the_native_first_run_is_set_up_natively() {
    for bytes in ["", "\n"] {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(".gemini/config/mcp_config.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, bytes).expect("empty config");
        let configured = ensure(
            &temp.path().join("project"),
            &environment(temp.path()),
            || {
                write(&path, &json!({"mcpServers":{"cccc":ready()}}));
                Ok(())
            },
        )
        .expect("native setup fills the empty file");
        assert_eq!(configured, path);
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
    let hooks = temp.path().join(".gemini/config/hooks.json");
    let malformed = b"{broken";
    std::fs::create_dir_all(settings.parent().expect("settings dir")).expect("settings dir");
    std::fs::write(&settings, malformed).expect("existing settings");
    std::fs::write(&hooks, malformed).expect("existing hook configuration");
    ensure(
        &temp.path().join("project"),
        &environment(temp.path()),
        || panic!("no setup"),
    )
    .expect("read-only startup");
    assert!(!path.with_extension("cccc.lock").exists());
    assert!(!settings.with_extension("cccc.lock").exists());
    assert!(!hooks.with_extension("cccc.lock").exists());
    assert_eq!(std::fs::read(&settings).expect("read settings"), malformed);
    assert_eq!(
        std::fs::read(&hooks).expect("read hook configuration"),
        malformed
    );
}

#[test]
fn setup_does_not_create_or_modify_user_settings_json() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join(".gemini/config/mcp_config.json");
    let settings = temp.path().join(".gemini/antigravity-cli/settings.json");

    ensure(
        &temp.path().join("project"),
        &environment(temp.path()),
        || {
            write(&path, &json!({"mcpServers":{"cccc":ready()}}));
            Ok(())
        },
    )
    .expect("setup without creating settings.json");
    assert!(!settings.exists());
    assert!(!settings.with_extension("cccc.lock").exists());

    let user_prefs = br#"{"showFeedbackSurvey":true,"model":"chosen-model"}"#;
    std::fs::create_dir_all(settings.parent().expect("settings dir")).expect("settings dir");
    std::fs::write(&settings, user_prefs).expect("write user prefs");
    ensure(
        &temp.path().join("project"),
        &environment(temp.path()),
        || panic!("already configured"),
    )
    .expect("setup preserves user settings.json");
    assert_eq!(
        std::fs::read(&settings).expect("read user prefs"),
        user_prefs
    );
    assert!(!settings.with_extension("cccc.lock").exists());
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

#[test]
fn windows_mixed_case_userprofile_override_aligns_mcp_setup_and_onboarding_gate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home_a = temp.path().join("home-a");
    let home_b = temp.path().join("home-b");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&home_a).expect("home a");
    std::fs::create_dir_all(&home_b).expect("home b");
    let inherited = vec![("USERPROFILE".to_owned(), home_a.display().to_string())];
    let actor_env = BTreeMap::from([("userprofile".to_owned(), home_b.display().to_string())]);
    let resolved = cccc_runtime::resolve_antigravity_home_from(&cwd, &actor_env, inherited, true)
        .expect("resolved windows home");
    assert_eq!(
        resolved, home_b,
        "case-insensitive Windows override must resolve both setup and gate to home B"
    );
    let mcp_b = home_b.join(".gemini/config/mcp_config.json");
    let configured = ensure_in_home(&cwd, &resolved, || {
        write(&mcp_b, &json!({"mcpServers":{"cccc":ready()}}));
        Ok(())
    })
    .expect("setup in home B");
    assert_eq!(configured, mcp_b);
    assert!(
        !home_a.join(".gemini/config/mcp_config.json").exists(),
        "inherited USERPROFILE=A must not be touched when actor sets userprofile=B"
    );
    assert_eq!(
        resolved.join(".gemini/antigravity-cli/cache/onboarding.json"),
        home_b.join(".gemini/antigravity-cli/cache/onboarding.json")
    );
}
