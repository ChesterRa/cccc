use super::*;
use crate::connect_transport::{PeerClient, tests::send_request};
use cccc_core::GroupStore;

#[tokio::test]
async fn permanent_key_connects_multiple_groups_and_deletion_blocks_channels_and_reconnects() {
    let temp = tempfile::tempdir().expect("temp");
    let homes: [HomeLayout; 3] = std::array::from_fn(|index| {
        HomeLayout::from_path(temp.path().join(index.to_string())).expect("home")
    });
    let mut groups = Vec::new();
    for (index, home) in homes.iter().enumerate() {
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create(&format!("Group {index}"), "").expect("group");
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("worker"))
            .expect("actor metadata only");
        store.save(&group).expect("save");
        groups.push(group.group_id);
    }
    let reserve = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("port");
    let address = reserve.local_addr().expect("address").to_string();
    direct::configure(
        &homes[0],
        Some(DirectListener {
            bind: address.clone(),
            address,
        }),
        None,
    )
    .expect("listener");
    let text = direct::shared::create(&homes[0], &groups[0], None, 0).expect("invitation");
    let root = direct::load(&homes[0]).expect("store").shared_invitations[0]
        .invitation
        .id
        .clone();
    let ids = [
        direct::join(&homes[1], &groups[1], &text).expect("first join"),
        direct::join(&homes[2], &groups[2], &text).expect("second join"),
    ];
    assert_ne!(ids[0], ids[1]);
    let channels: [Channels; 3] = std::array::from_fn(|_| Channels::default());
    let locks = DispatchLocks::default();
    let mut services = JoinSet::new();
    drop(reserve);
    for index in 0..3 {
        services.spawn(run(
            homes[index].clone(),
            locks.clone(),
            channels[index].clone(),
        ));
    }
    tests::wait(|| direct::load(&homes[0]).expect("store").relations.len() == 2).await;
    for id in &ids {
        direct::set_state(&homes[0], &groups[0], id, true).expect("approve each exact Group");
    }
    tests::wait(|| {
        channels[0]
            .0
            .try_lock()
            .is_ok_and(|all| ids.iter().all(|id| all.contains_key(id)))
    })
    .await;
    assert_eq!(
        direct::load(&homes[0]).expect("store").shared_invitations[0].uses,
        2
    );
    let client = PeerClient {
        http: None,
        direct: channels[0].clone(),
    };
    for index in 1..3 {
        let peer = InstanceIdentity::load(&homes[index])
            .expect("identity")
            .peer_id;
        crate::connect_transport::refresh_catalog_scoped(
            &homes[0],
            &client,
            &peer,
            Some(&ids[index - 1]),
        )
        .await
        .expect("authorized catalog");
        let request = send_request(
            &groups[0],
            &groups[index],
            &peer,
            json!(["worker"]),
            &format!("shared-{index}"),
        );
        let sent = crate::dispatch::dispatch(&homes[0], &request);
        assert!(sent.ok, "{sent:?}");
        crate::connect_transport::delivery::process(
            &homes[0],
            &client,
            &locks,
            &peer,
            sent.result["delivery_id"].as_str().expect("delivery"),
        )
        .await
        .expect("real TLS delivery");
        let store = GroupStore::new(homes[index].clone()).expect("store");
        let events = cccc_core::ledger::read_all(&store.ledger_path(&groups[index]).expect("path"))
            .expect("events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "chat.message")
                .count(),
            1
        );
    }
    let stale = [
        direct::load(&homes[1]).expect("guest").relations[0].clone(),
        direct::load(&homes[2]).expect("guest").relations[0].clone(),
    ];
    direct::shared::revoke(&homes[0], &groups[0], &root).expect("delete key and revoke its grants");
    for index in 1..3 {
        let peer = InstanceIdentity::load(&homes[index])
            .expect("identity")
            .peer_id;
        assert!(direct::binding(&homes[0], &peer, &ids[index - 1]).is_err());
    }
    tests::wait(|| {
        channels[0].0.try_lock().is_ok_and(|all| {
            ids.iter()
                .all(|id| all.get(id).is_none_or(|sender| sender.is_closed()))
        })
    })
    .await;
    services.abort_all();
    while services.join_next().await.is_some() {}
    let host_channels = Channels::default();
    services.spawn(run(homes[0].clone(), locks.clone(), host_channels.clone()));
    tests::wait(|| {
        std::net::TcpStream::connect(
            direct::load(&homes[0])
                .expect("listener")
                .listener
                .expect("listener")
                .address,
        )
        .is_ok()
    })
    .await;
    for index in 1..3 {
        // A stale client may still believe its grant is active. The new TLS
        // handshake must obtain the revocation from the host before any channel.
        direct::update(&homes[index], |store| {
            store.relations[0].state = DirectState::Active;
            Ok(())
        })
        .expect("stale local snapshot");
        let guest_channels = Channels::default();
        handshake::dial(
            homes[index].clone(),
            locks.clone(),
            guest_channels.clone(),
            stale[index - 1].clone(),
            &mut RetryState::default(),
        )
        .await
        .expect("TLS handshake returns a definitive rejection");
        assert_eq!(
            direct::load(&homes[index])
                .expect("remote rejection")
                .relations[0]
                .state,
            DirectState::Revoked
        );
        assert!(guest_channels.0.lock().await.is_empty());
    }
    services.abort_all();
    while services.join_next().await.is_some() {}
}
