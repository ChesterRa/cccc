use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, Registry, active, group_scope, scope};
use serde_json::{Value, json};

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("args").clone(),
        },
    )
}

fn success(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

#[test]
fn nonactivating_attach_preserves_all_defaults_and_supports_actor_pins() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let target = store.create("target", "").expect("target");
    let owner = store.create("owner", "").expect("owner");
    let scopes = ["initial", "owned", "unclaimed"].map(|name| {
        let path = temp.path().join(name);
        std::fs::create_dir(&path).expect("directory");
        scope::detect(&path).expect("scope")
    });
    group_scope::attach(&store, &target.group_id, scopes[0].clone()).expect("target scope");
    group_scope::attach(&store, &owner.group_id, scopes[1].clone()).expect("owned scope");
    active::set(&home, &owner.group_id).expect("global selection");
    success(
        &home,
        "actor_add",
        json!({"group_id":target.group_id,"actor_id":"managed","enabled":false,"default_scope_key":"","by":"user"}),
    );
    let before = store.load(&target.group_id).expect("before");
    let registry_before = Registry::load(&home).expect("registry before");

    for added in [&scopes[1], &scopes[1], &scopes[2]] {
        let receipt = success(
            &home,
            "attach",
            json!({"group_id":target.group_id,"path":added.url,"activate":false,"by":"user"}),
        );
        assert_eq!(receipt["scope_key"], added.scope_key);
        let current = store.load(&target.group_id).expect("attached group");
        assert_eq!(current.active_scope_key, before.active_scope_key);
        assert_eq!(current.actors, before.actors, "attach must not save Actors");
        let registry = Registry::load(&home).expect("registry");
        assert_eq!(registry.defaults, registry_before.defaults);
        for id in [&target.group_id, &owner.group_id] {
            assert_eq!(
                registry.groups[id].default_scope_key,
                registry_before.groups[id].default_scope_key
            );
        }
        assert_eq!(
            active::get(&home).expect("active"),
            Some(owner.group_id.clone())
        );
        assert_eq!(
            receipt["scope"],
            serde_json::to_value(added).expect("scope")
        );
    }
    assert_eq!(store.load(&target.group_id).expect("group").scopes.len(), 3);
    let events = cccc_core::ledger::read_all(&store.ledger_path(&target.group_id).expect("ledger"))
        .expect("events");
    for event in events.iter().filter(|event| event.kind == "group.attach") {
        assert_ne!(event.scope_key, before.active_scope_key);
        assert_eq!(event.data["activate"], false);
    }

    let updated = success(
        &home,
        "actor_update",
        json!({"group_id":target.group_id,"actor_id":"managed","patch":{"default_scope_key":scopes[2].scope_key},"by":"user"}),
    );
    assert_eq!(updated["actor"]["default_scope_key"], scopes[2].scope_key);
    assert_eq!(updated["actor"]["enabled"], false);
    let created = success(
        &home,
        "actor_add",
        json!({"group_id":target.group_id,"actor_id":"pinned","enabled":false,"default_scope_key":scopes[1].scope_key,"by":"user"}),
    );
    assert_eq!(created["actor"]["default_scope_key"], scopes[1].scope_key);
    assert_eq!(
        store
            .load(&target.group_id)
            .expect("group")
            .active_scope_key,
        scopes[0].scope_key
    );
    assert_eq!(
        Registry::load(&home).expect("registry").defaults,
        registry_before.defaults
    );
    assert_eq!(
        active::get(&home).expect("active"),
        Some(owner.group_id.clone())
    );

    // Ordinary attach still claims the default and selects the Group.
    success(
        &home,
        "attach",
        json!({"group_id":target.group_id,"path":scopes[2].url,"by":"user"}),
    );
    assert_eq!(
        store
            .load(&target.group_id)
            .expect("group")
            .active_scope_key,
        scopes[2].scope_key
    );
    assert_eq!(
        Registry::load(&home).expect("registry").defaults[&scopes[2].scope_key],
        target.group_id
    );
    assert_eq!(active::get(&home).expect("active"), Some(target.group_id));
}

#[test]
fn first_nonactivating_attachment_does_not_invent_an_active_scope_or_group() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("empty", "").expect("group");
    let registry = Registry::load(&home).expect("registry");
    let receipt = success(
        &home,
        "attach",
        json!({"group_id":group.group_id,"path":temp.path(),"activate":false,"by":"user"}),
    );
    let attached = store.load(&group.group_id).expect("attached group");
    assert_eq!(attached.scopes.len(), 1);
    assert_eq!(
        attached.scopes[0].scope_key,
        receipt["scope_key"].as_str().expect("added key")
    );
    assert!(attached.active_scope_key.is_empty());
    assert_eq!(
        Registry::load(&home).expect("registry").defaults,
        registry.defaults
    );
    assert!(
        Registry::load(&home).expect("registry").groups[&group.group_id]
            .default_scope_key
            .is_empty()
    );
    assert!(active::get(&home).expect("active Group").is_none());
}

#[test]
fn nonactivating_attach_validates_options_paths_and_authority_before_writes() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("target", "").expect("group");
    for actor_id in ["lead", "peer"] {
        success(
            &home,
            "actor_add",
            json!({"group_id":group.group_id,"actor_id":actor_id,"enabled":false,"by":"user"}),
        );
    }
    let file = temp.path().join("not-a-directory");
    std::fs::write(&file, "fixture").expect("file");
    let before = store.load(&group.group_id).expect("before");
    let registry = Registry::load(&home).expect("registry");
    let ledger =
        std::fs::read(store.ledger_path(&group.group_id).expect("ledger")).expect("ledger");
    for (args, code) in [
        (
            json!({"group_id":group.group_id,"path":temp.path(),"activate":"false","by":"user"}),
            "invalid_args",
        ),
        (
            json!({"path":temp.path(),"activate":false,"by":"user"}),
            "invalid_args",
        ),
        (
            json!({"group_id":group.group_id,"activate":false,"by":"user"}),
            "invalid_args",
        ),
        (
            json!({"group_id":group.group_id,"path":"","activate":false,"by":"user"}),
            "invalid_args",
        ),
        (
            json!({"group_id":group.group_id,"path":file,"activate":false,"by":"user"}),
            "invalid_args",
        ),
        (
            json!({"group_id":group.group_id,"path":temp.path().join("missing"),"activate":false,"by":"user"}),
            "invalid_args",
        ),
        (
            json!({"group_id":group.group_id,"path":temp.path().join("missing"),"activate":false,"by":"peer"}),
            "permission_denied",
        ),
    ] {
        let response = call(&home, "attach", args);
        assert!(!response.ok);
        assert_eq!(response.error.expect("error").code, code);
        assert_eq!(
            store.load(&group.group_id).expect("unchanged group"),
            before
        );
        assert_eq!(Registry::load(&home).expect("unchanged registry"), registry);
        assert_eq!(
            std::fs::read(store.ledger_path(&group.group_id).expect("ledger")).expect("ledger"),
            ledger
        );
    }
}

#[test]
fn nonactivating_attach_rejects_a_scope_collision_but_ordinary_attach_can_rebind() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("target", "").expect("group");
    let projects = ["clone-a", "clone-b"].map(|name| temp.path().join(name));
    for project in &projects {
        std::fs::create_dir(project).expect("directory");
        for args in [
            vec!["init", "--quiet"],
            vec![
                "remote",
                "add",
                "origin",
                "https://fixture.invalid/same-project.git",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(project)
                    .status()
                    .expect("fixture git")
                    .success()
            );
        }
    }
    let first = scope::detect(&projects[0]).expect("first scope");
    let second = scope::detect(&projects[1]).expect("second scope");
    assert_eq!(first.scope_key, second.scope_key);
    group_scope::attach(&store, &group.group_id, first).expect("first attach");
    let before = store.load(&group.group_id).expect("persisted snapshot");
    let registry = Registry::load(&home).expect("registry");
    let ledger =
        std::fs::read(store.ledger_path(&group.group_id).expect("ledger")).expect("ledger");
    let rejected = call(
        &home,
        "attach",
        json!({"group_id":group.group_id,"path":projects[1],"activate":false,"by":"user"}),
    );
    assert!(
        !rejected.ok,
        "nonactivating attach must not replace an attached root"
    );
    assert_eq!(rejected.error.expect("conflict").code, "scope_conflict");
    assert_eq!(
        store.load(&group.group_id).expect("unchanged group"),
        before
    );
    assert_eq!(Registry::load(&home).expect("unchanged registry"), registry);
    assert_eq!(
        std::fs::read(store.ledger_path(&group.group_id).expect("ledger")).expect("ledger"),
        ledger
    );

    // Both legacy omission and explicit activation retain deliberate rebinding.
    for (project, activate) in [(&projects[1], None), (&projects[0], Some(true))] {
        let mut args = json!({"group_id":group.group_id,"path":project,"by":"user"});
        if let Some(activate) = activate {
            args["activate"] = json!(activate);
        }
        let receipt = success(&home, "attach", args);
        let rebound = store.load(&group.group_id).expect("rebound group");
        assert_eq!(rebound.scopes.len(), 1);
        assert_eq!(
            rebound.scopes[0].url,
            project.canonicalize().expect("root").to_string_lossy()
        );
        assert_eq!(
            rebound.active_scope_key,
            receipt["scope_key"].as_str().expect("key")
        );
        assert_eq!(
            Registry::load(&home).expect("registry").defaults[&rebound.active_scope_key],
            group.group_id
        );
        assert_eq!(
            active::get(&home).expect("active"),
            Some(group.group_id.clone())
        );
    }
}
