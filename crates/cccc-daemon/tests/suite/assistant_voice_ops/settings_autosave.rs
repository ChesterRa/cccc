use super::*;

#[test]
fn recognition_settings_save_without_starting_an_enabled_but_stopped_actor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("voice", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            let mut foreman = Actor::new("foreman");
            foreman.role = Some(ActorRole::Foreman);
            foreman.command = vec!["/cccc/command/that/does/not/exist".into()];
            doc.actors.push(foreman);
            Ok(())
        })
        .expect("foreman");
    ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"enabled":true}}),
    );
    store
        .mutate(&group.group_id, |doc| {
            doc.running = true;
            Ok(())
        })
        .expect("running group");
    let saved = ok(
        &home,
        "assistant_settings_update",
        json!({"group_id":group.group_id,"patch":{"config":{"recognition_backend":"external_provider_asr","external_asr_provider":"volcengine","auto_document_max_window_seconds":45}}}),
    );
    assert_eq!(saved.result["actor_started"], false);
    assert_eq!(saved.result["assistant"]["enabled"], true);
    let state = store.load(&group.group_id).expect("saved group");
    let config = &state.extra["assistants"]["voice_secretary"]["config"];
    assert_eq!(config["recognition_backend"], "external_provider_asr");
    assert_eq!(config["external_asr_provider"], "volcengine");
    assert_eq!(config["auto_document_max_window_seconds"], 45);
    assert_eq!(
        state
            .actors
            .iter()
            .filter(|actor| actor.id == "voice-secretary")
            .count(),
        1
    );
}
