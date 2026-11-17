use super::*;

fn actor() -> ActorRef {
    ActorRef {
        group_id: "g_mcp_registry".into(),
        actor_id: "peer".into(),
        actor_created_at: "created".into(),
        generation: None,
    }
}

fn group() -> GroupDoc {
    serde_json::from_value(serde_json::json!({
        "v":1,"group_id":"g_mcp_registry","generation":"resource-generation",
        "title":"MCP fixture","created_at":"created","updated_at":"created"
    }))
    .expect("group fixture")
}

fn registry() -> (tempfile::TempDir, Registry) {
    let temp = tempfile::tempdir().expect("isolated binding home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let registry = Registry::new(
        home,
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::new_v4().simple().to_string(),
    );
    (temp, registry)
}

fn report(binding: &Binding, source: char) -> Registration {
    Registration {
        registration: cccc_core::owned_mcp::REGISTRATION_API,
        instance: binding.instance.clone(),
        launch_id: binding.launch_id.clone(),
        version: "0.4.58".into(),
        source_id: source.to_string().repeat(64),
        mcp_contract: cccc_contracts::MCP_DAEMON_CONTRACT,
    }
}

#[test]
fn a_registration_cannot_invent_controlled_launch_provenance() {
    let (_temp, mut registry) = registry();
    let actor = actor();
    registry
        .reserve(&actor, &group(), false)
        .expect("uncontrolled launch");
    let binding = registry
        .binding(&actor.group_id, &actor.actor_id)
        .expect("binding");
    registry
        .register(
            NativeHost::capture(std::process::id()).expect("native"),
            report(&binding, 'a'),
        )
        .expect("observed sample");
    let sample = registry.snapshot(&actor).expect("snapshot");
    assert!(sample.initialized);
    assert_eq!(sample.source_controlled, Some(false));
    registry.forget(&actor);
    registry
        .reserve(&actor, &group(), true)
        .expect("controlled next launch");
    let sample = registry.snapshot(&actor).expect("next snapshot");
    assert_eq!(sample.source_controlled, Some(true));
    assert!(
        !sample.initialized,
        "new provenance does not relabel an old MCP"
    );
    assert_eq!(sample.processes.len(), 1);
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn reconnects_and_new_launches_cannot_erase_a_live_older_mcp() {
    let (_temp, mut registry) = registry();
    let actor = actor();
    registry
        .reserve(&actor, &group(), true)
        .expect("publish initial binding");
    let first = registry
        .binding(&actor.group_id, &actor.actor_id)
        .expect("reserved binding");
    let native = NativeHost::capture(std::process::id()).expect("test native identity");
    let old = report(&first, 'a');
    registry
        .register(native.clone(), old.clone())
        .expect("first registration");
    registry
        .register(native.clone(), old.clone())
        .expect("same process may repeat its registration");
    assert_eq!(
        registry.snapshot(&actor).expect("snapshot").processes.len(),
        1
    );
    assert_eq!(
        registry
            .register(native.clone(), report(&first, 'b'))
            .expect_err("a changed report must not overwrite a live image")
            .code,
        "mcp_identity_changed"
    );

    registry.forget(&actor);
    registry
        .reserve(&actor, &group(), true)
        .expect("publish next binding");
    let second = registry
        .binding(&actor.group_id, &actor.actor_id)
        .expect("new binding");
    assert_ne!(first.launch_id, second.launch_id);
    let before = registry.snapshot(&actor).expect("new launch snapshot");
    assert!(!before.initialized);
    assert_eq!(before.processes.len(), 1);
    assert_eq!(before.processes[0].report, old);
    assert_eq!(
        registry
            .register(native, old)
            .expect_err("a retired binding cannot initialize new tools")
            .code,
        "mcp_launch_unknown"
    );

    let mut child = Child(
        cccc_runtime::background_command("sleep")
            .arg("30")
            .spawn()
            .expect("local fixture process"),
    );
    let second_native = NativeHost::capture(child.0.id()).expect("second native identity");
    registry
        .register(second_native.clone(), report(&second, 'b'))
        .expect("second MCP instance");
    let both = registry.snapshot(&actor).expect("both instances");
    assert!(both.initialized);
    assert_eq!(both.processes.len(), 2);
    assert!(both.processes.iter().all(|process| process.observable));
    child.0.kill().expect("end fixture");
    child.0.wait().expect("reap fixture");
    assert!(second_native.has_exited());
    registry.refresh();
    let remaining = registry.snapshot(&actor).expect("pruned instances");
    assert_eq!(remaining.processes.len(), 1);
    assert_eq!(remaining.processes[0].report.launch_id, first.launch_id);
}

#[test]
fn an_unknown_or_incompatible_report_does_not_initialize_the_launch() {
    let (_temp, mut registry) = registry();
    let actor = actor();
    registry
        .reserve(&actor, &group(), true)
        .expect("publish binding");
    let binding = registry
        .binding(&actor.group_id, &actor.actor_id)
        .expect("reserved binding");
    let native = NativeHost::capture(std::process::id()).expect("native identity");
    let mut incompatible = report(&binding, 'a');
    incompatible.registration += 1;
    assert_eq!(
        registry
            .register(native.clone(), incompatible)
            .expect_err("unknown registration API")
            .code,
        "invalid_mcp_registration"
    );
    let mut unknown = report(&binding, 'a');
    unknown.launch_id = uuid::Uuid::new_v4().simple().to_string();
    assert_eq!(
        registry
            .register(native, unknown)
            .expect_err("unknown launch")
            .code,
        "mcp_launch_unknown"
    );
    let snapshot = registry.snapshot(&actor).expect("launch remains unknown");
    assert!(!snapshot.initialized);
    assert!(snapshot.processes.is_empty());
}

#[test]
fn exact_resumes_read_a_new_binding_from_the_same_locator() {
    let (_temp, mut registry) = registry();
    let actor = actor();
    let group = group();
    registry
        .reserve(&actor, &group, true)
        .expect("first launch");
    let file = registry
        .file(&actor.group_id, &actor.actor_id)
        .expect("locator");
    let first = Binding::read_file(&file).expect("first published binding");
    assert_eq!(
        registry.binding(&actor.group_id, &actor.actor_id),
        Some(first.clone())
    );
    registry.forget(&actor);
    assert!(
        file.is_file(),
        "Stop must preserve the descriptor's locator"
    );
    registry
        .reserve(&actor, &group, true)
        .expect("resume launch");
    assert_eq!(
        registry.file(&actor.group_id, &actor.actor_id),
        Some(file.clone())
    );
    let next = Binding::read_file(&file).expect("current contents, unchanged descriptor");
    assert_ne!(first.launch_id, next.launch_id);
    assert_eq!(
        registry.binding(&actor.group_id, &actor.actor_id),
        Some(next)
    );
    assert_eq!(
        registry
            .register(
                NativeHost::capture(std::process::id()).expect("native identity"),
                report(&first, 'a')
            )
            .expect_err("a stale descriptor value still fails closed")
            .code,
        "mcp_launch_unknown"
    );
}

#[test]
fn failed_publication_does_not_reserve_a_launch() {
    let (_temp, mut registry) = registry();
    let actor = actor();
    let group = group();
    let home = &registry.owner.as_ref().expect("actual owner").0;
    let file = binding_file(home, &group, &actor.actor_id, &actor.actor_created_at).expect("path");
    let parent = file.parent().expect("binding directory");
    std::fs::create_dir_all(parent.parent().expect("state directory")).expect("state");
    std::fs::write(parent, "blocks binding publication").expect("fault fixture");
    assert_eq!(
        registry
            .reserve(&actor, &group, true)
            .expect_err("unwritable publication")
            .code,
        "mcp_binding_unavailable"
    );
    assert!(registry.snapshot(&actor).is_none());
    assert!(registry.file(&actor.group_id, &actor.actor_id).is_none());
    assert_eq!(
        std::fs::read_to_string(parent).expect("fault remains"),
        "blocks binding publication"
    );
}
