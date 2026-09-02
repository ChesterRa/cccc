use super::*;
use cccc_contracts::{ActorRuntime, CodexVoiceAnalystSettings};
use serde_json::json;

#[test]
fn rejects_invalid_runtime_inputs() {
    assert!(
        normalize(CodexVoiceAnalystSettings {
            command: vec!["codex".into(), "".into()],
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        normalize(CodexVoiceAnalystSettings {
            profile_id: "bad/profile".into(),
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        patched_private_environment(
            &BTreeMap::new(),
            BTreeMap::from([("CCCC_HOME".into(), "other".into())]),
            &[],
        )
        .is_err()
    );
    assert!(
        validate_private_environment(&BTreeMap::from([(
            "CODEX_HOME".into(),
            "relative/codex".into()
        )]))
        .is_err()
    );
    assert!(
        patched_private_environment(
            &BTreeMap::new(),
            BTreeMap::from([("CODEX_HOME".into(), "relative/codex".into())]),
            &[],
        )
        .is_err()
    );
}

#[test]
fn normalizes_the_shared_runtime_command() {
    let normalized = normalize(CodexVoiceAnalystSettings {
        command: vec![" codex ".into(), " --search ".into()],
        ..Default::default()
    })
    .expect("normalized command");
    assert_eq!(normalized.command, ["codex", "--search"]);
}

#[test]
fn runtime_identity_tracks_codex_storage_roots_but_not_model_credentials() {
    let baseline = ResolvedAgentRuntime {
        runtime: ActorRuntime::Codex,
        command: vec!["codex".into()],
        environment: BTreeMap::from([
            ("CODEX_HOME".into(), "/tmp/codex-a".into()),
            ("PROVIDER_API_KEY".into(), "first".into()),
        ]),
    };
    let mut provider_change = baseline.clone();
    provider_change.command = vec!["codex".into(), "--model".into(), "other".into()];
    provider_change
        .environment
        .insert("PROVIDER_API_KEY".into(), "second".into());
    assert_eq!(
        baseline.identity_fingerprint(),
        provider_change.identity_fingerprint()
    );

    let mut identity_change = baseline.clone();
    identity_change
        .environment
        .insert("CODEX_HOME".into(), "/tmp/codex-b".into());
    assert_ne!(
        baseline.identity_fingerprint(),
        identity_change.identity_fingerprint()
    );
    assert!(identity_environment_changed(
        &baseline.environment,
        &identity_change.environment
    ));
}

#[test]
fn stores_private_environment_outside_settings_yaml() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let values = BTreeMap::from([("OPENAI_API_KEY".into(), "secret".into())]);
    replace_private_environment(&home, &values).expect("save secrets");
    assert_eq!(private_environment(&home).expect("load secrets"), values);
    assert!(!home.root().join("settings.yaml").exists());
}

#[test]
fn creates_stable_neutral_workdir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let first = workdir(&home).expect("workdir");
    let second = workdir(&home).expect("same workdir");
    assert_eq!(first, second);
    assert!(first.ends_with("state/codex_voice/analyst-workdir"));
}

#[test]
fn resolves_the_same_runtime_profile_shape_used_by_actors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let profile = ProfileStore::new(home.clone()).expect("profiles");
    profile
        .upsert(
            json!({
                "id":"voice-codex",
                "name":"Voice Codex",
                "runtime":"codex",
                "runner":"pty",
                "command":["codex","--profile","voice"],
                "submit":"enter"
            })
            .as_object()
            .expect("profile")
            .clone(),
            None,
        )
        .expect("save profile");
    profile
        .replace_secrets(
            "voice-codex",
            BTreeMap::from([(
                "CODEX_HOME".into(),
                temp.path().join("codex").to_string_lossy().into_owned(),
            )]),
        )
        .expect("profile secrets");
    let resolved = resolve(
        &home,
        &CodexVoiceAnalystSettings {
            profile_id: "voice-codex".into(),
            ..Default::default()
        },
        &BTreeMap::new(),
    )
    .expect("resolved profile");
    assert_eq!(resolved.runtime, ActorRuntime::Codex);
    assert_eq!(resolved.command, ["codex", "--profile", "voice"]);
    assert!(resolved.environment.contains_key("CODEX_HOME"));
}

#[test]
fn rejects_a_profile_until_its_runtime_has_a_voice_adapter() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    ProfileStore::new(home.clone())
        .expect("profiles")
        .upsert(
            json!({
                "id":"claude",
                "runtime":"claude",
                "runner":"pty",
                "command":["claude"],
                "submit":"enter"
            })
            .as_object()
            .expect("profile")
            .clone(),
            None,
        )
        .expect("save profile");
    let error = resolve(
        &home,
        &CodexVoiceAnalystSettings {
            profile_id: "claude".into(),
            ..Default::default()
        },
        &BTreeMap::new(),
    )
    .expect_err("unsupported runtime");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}
