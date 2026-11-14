use super::*;
use crate::{GroupStore, HomeLayout, actors, local_sessions::NativeHost};
use cccc_contracts::{
    Actor, ActorRuntime,
    local_session::{LocalSessionBinding, LocalSessionRuntime, LocalSessionSource},
    session_node::*,
};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("private Home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
        home.initialize().expect("initialize");
        register_endpoint(
            &home,
            SessionNodeEndpoint {
                endpoint_id: "hub".into(),
                addresses: vec!["127.0.0.1:2345".into()],
                relay_url: None,
            },
            NativeHost::capture(std::process::id()).expect("live test owner"),
            "gateway",
        )
        .expect("gateway");
        Self { _temp: temp, home }
    }
    fn pair(&self, node: &str) -> SessionNodePeer {
        let invite = create_invite(&self.home).expect("invitation");
        pair(
            &self.home,
            "gateway",
            node,
            node,
            Some((&invite.invite_id, &invite.secret)),
        )
        .expect("pair")
    }
    fn sources(&self, node: &str) -> Vec<RemoteSessionSource> {
        update_sources(&self.home, "gateway", node, vec![exported()]).expect("source advertisement")
    }
}

fn exported() -> SessionNodeSource {
    SessionNodeSource {
        source: LocalSessionSource {
            source_id: "codex:same-native-id".into(),
            runtime: LocalSessionRuntime::Codex,
            native_session_id: "same-native-id".into(),
            cwd: "/not/a/path/on/the/hub".into(),
            title: "Native session".into(),
            origin: Some("cli".into()),
            connected: true,
        },
        session_revision: "native-revision".into(),
        alive: true,
    }
}

#[test]
fn invitations_enroll_one_identity_and_revocation_requires_a_new_grant() {
    let f = Fixture::new();
    let invite = create_invite(&f.home).expect("invitation");
    let original = pair(
        &f.home,
        "gateway",
        "node-a",
        "Laptop",
        Some((&invite.invite_id, &invite.secret)),
    )
    .expect("first pairing");
    assert!(
        pair(
            &f.home,
            "gateway",
            "node-b",
            "Other",
            Some((&invite.invite_id, &invite.secret))
        )
        .is_err()
    );
    assert_eq!(
        pair(&f.home, "gateway", "node-a", "Laptop", None)
            .expect("authenticated resume")
            .grant_id,
        original.grant_id
    );
    let old_source = f.sources("node-a").remove(0);
    revoke(&f.home, "node-a").expect("revoke");
    assert!(source(&f.home, &old_source.source.source_id).is_err());
    assert!(pair(&f.home, "gateway", "node-a", "Laptop", None).is_err());
    let replacement = f.pair("node-a");
    assert_ne!(replacement.grant_id, original.grant_id);
    assert_ne!(
        f.sources("node-a")[0].source.source_id,
        old_source.source.source_id
    );
    assert!(source(&f.home, &old_source.source.source_id).is_err());
}

#[test]
fn sources_are_namespaced_and_bad_or_missing_advertisements_cannot_retain_authority() {
    let f = Fixture::new();
    f.pair("node-a");
    f.pair("node-b");
    let a = f.sources("node-a").remove(0);
    let b = f.sources("node-b").remove(0);
    assert_ne!(a.source.source_id, b.source.source_id);
    let before = std::fs::read(store::directory(&f.home).join("state.json")).expect("state");
    assert!(update_sources(&f.home, "wrong-gateway", "node-a", vec![exported()]).is_err());
    let mut invalid = exported();
    invalid.source.source_id = a.source.source_id.clone();
    assert!(update_sources(&f.home, "gateway", "node-a", vec![invalid]).is_err());
    assert_eq!(
        std::fs::read(store::directory(&f.home).join("state.json")).expect("state"),
        before
    );
    update_sources(&f.home, "gateway", "node-a", vec![]).expect("source removed");
    assert!(source(&f.home, &a.source.source_id).is_err());
    assert!(source(&f.home, &b.source.source_id).is_ok());
    let mut claude = exported();
    claude.source.runtime = LocalSessionRuntime::Claude;
    claude.source.source_id = "claude:host:123:456".into();
    claude.source.native_session_id = "different-conversation-id".into();
    assert_eq!(
        update_sources(&f.home, "gateway", "node-a", vec![claude])
            .expect("Claude host and conversation IDs differ")
            .len(),
        1
    );
}

#[test]
fn binding_requires_the_live_native_revision_and_exact_enabled_actor_generation() {
    let f = Fixture::new();
    f.pair("node");
    let remote = f.sources("node").remove(0);
    let store = GroupStore::new(f.home.clone()).expect("store");
    let mut group = store.create("Remote Group", "").expect("Group");
    let mut actor = Actor::new("remote-member");
    actor.runtime = ActorRuntime::Codex;
    actor.external_session_id = remote.source.source_id.clone();
    let actor = actors::add(&mut group, actor).expect("member");
    store.save(&group).expect("save");
    let binding = LocalSessionBinding {
        group_id: group.group_id.clone(),
        actor_id: actor.id,
        actor_generation: actors::generation_identity(&group.actors[0]),
        source_id: remote.source.source_id,
        session_revision: remote.session_revision,
    };
    assert!(crate::local_sessions::validate_binding(&f.home, &binding).is_ok());
    let mut stale = binding.clone();
    stale.session_revision = "other".into();
    assert!(binding_valid(&f.home, &stale).is_err());
    group.actors[0].enabled = false;
    store.save(&group).expect("pause");
    assert!(binding_valid(&f.home, &binding).is_err());
    group.actors[0].enabled = true;
    group.actors[0].generation = "replacement".into();
    store.save(&group).expect("replace");
    assert!(binding_valid(&f.home, &binding).is_err());
    group.actors[0].generation = binding.actor_generation.clone();
    store.save(&group).expect("restore fixture");
    set_connected(&f.home, "gateway", "node", false).expect("disconnect");
    assert!(binding_valid(&f.home, &binding).is_err());
}

#[test]
fn stale_gateway_and_expired_ticket_fail_closed_without_sleeping() {
    let f = Fixture::new();
    let invite = create_invite(&f.home).expect("invitation");
    store::update(&f.home, |state| {
        state
            .invites
            .get_mut(&invite.invite_id)
            .expect("invitation")
            .expires_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("expired fixture");
    assert!(
        pair(
            &f.home,
            "gateway",
            "node",
            "Laptop",
            Some((&invite.invite_id, &invite.secret))
        )
        .is_err()
    );
    f.pair("node");
    let source_id = f.sources("node")[0].source.source_id.clone();
    store::update(&f.home, |state| {
        state
            .sources
            .get_mut(&source_id)
            .expect("source")
            .updated_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("expired source lease");
    assert!(source(&f.home, &source_id).is_err());
    clear_endpoint(&f.home, "wrong-gateway").expect("stale cleanup");
    assert!(endpoint(&f.home).expect("endpoint").is_some());
    clear_endpoint(&f.home, "gateway").expect("shutdown");
    assert_eq!(
        saved_endpoint(&f.home)
            .expect("saved endpoint")
            .expect("retained listen address")
            .addresses,
        ["127.0.0.1:2345"]
    );
    assert!(pair(&f.home, "gateway", "node", "Laptop", None).is_err());
}

#[path = "relay_tests.rs"]
mod relay_tests;

#[path = "grants_tests.rs"]
mod grants_tests;
