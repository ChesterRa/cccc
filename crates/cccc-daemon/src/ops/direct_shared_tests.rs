use super::*;

#[test]
fn reusable_invitation_actions_are_admin_only_and_polling_never_discloses_secrets() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Source", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer")).expect("actor");
    store.save(&group).expect("save");
    direct::configure(
        &home,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        None,
    )
    .expect("listener metadata");
    let call = |op: &str, args: Value| {
        crate::dispatch::dispatch(
            &home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("object").clone(),
            },
        )
    };
    let created = call(
        "connect_direct_invite",
        json!({"group_id":group.group_id,"by":"user","reusable":true}),
    );
    assert!(created.ok, "{created:?}");
    for seconds in [-1, 86_400, 604_800, 2_592_000] {
        let finite = call(
            "connect_direct_invite",
            json!({"group_id":group.group_id,"by":"user","reusable":true,"expires_in_seconds":seconds}),
        );
        assert!(!finite.ok, "finite duration {seconds} must be refused");
    }
    let legacy = call(
        "connect_direct_invite",
        json!({"group_id":group.group_id,"by":"user"}),
    );
    assert!(legacy.ok);
    let old = direct::decode_invitation(legacy.result["invitation"].as_str().expect("legacy key"))
        .expect("decode legacy");
    assert_eq!(old.v, 1);
    assert!(!old.reusable());
    let saved = direct::load(&home).expect("store");
    assert_eq!(saved.shared_invitations.len(), 1);
    let invitation = &saved.shared_invitations[0].invitation;
    direct::configure(
        &home,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "forward.example:14447".into(),
        }),
        None,
    )
    .expect("update saved endpoint");
    let status = call(
        "connect_direct_status",
        json!({"group_id":group.group_id,"by":"user"}),
    );
    assert!(status.ok);
    assert_eq!(status.result["capabilities"]["reusable_invitations"], true);
    assert_eq!(status.result["capabilities"]["permanent_invitations"], true);
    assert_eq!(status.result["capabilities"]["invitation_revocation"], true);
    assert_eq!(
        status.result["shared_invitations"][0]["expires_at"],
        "never"
    );
    assert_eq!(status.result["shared_invitations"][0]["uses"], 0);
    assert_eq!(
        status.result["shared_invitations"][0]["address"],
        "127.0.0.1:8847"
    );
    assert_eq!(
        status.result["listener"]["address"],
        "forward.example:14447"
    );
    let serialized = serde_json::to_string(&status.result).expect("status JSON");
    assert!(
        !serialized.contains(&invitation.secret),
        "polling must not disclose the secret"
    );
    assert!(
        !serialized.contains(created.result["invitation"].as_str().expect("text")),
        "polling must not disclose the bearer invitation"
    );
    for op in ["connect_direct_copy_invite", "connect_direct_delete_invite"] {
        let denied = call(
            op,
            json!({"group_id":group.group_id,"by":"peer","id":invitation.id}),
        );
        assert!(!denied.ok, "{op} must require administrator access");
    }
    let copy = call(
        "connect_direct_copy_invite",
        json!({"group_id":group.group_id,"by":"user","id":invitation.id}),
    );
    assert!(copy.ok);
    assert_eq!(copy.result["invitation"], created.result["invitation"]);
    let deleted = call(
        "connect_direct_delete_invite",
        json!({"group_id":group.group_id,"by":"user","id":invitation.id}),
    );
    assert!(deleted.ok);
    assert!(direct::shared::copy(&home, &group.group_id, &invitation.id).is_err());
}
