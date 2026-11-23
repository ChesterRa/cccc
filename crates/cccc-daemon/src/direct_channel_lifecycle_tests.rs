//! Exercise framing after the TLS boundary; the existing two-home test covers real TLS.
use super::*;
use cccc_core::GroupStore;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn broken_reader_reports_its_cause_and_releases_the_connection() {
    let temp = tempfile::tempdir().expect("temp");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
    let ga = GroupStore::new(a.clone())
        .expect("store")
        .create("A", "")
        .expect("group");
    let gb = GroupStore::new(b.clone())
        .expect("store")
        .create("B", "")
        .expect("group");
    direct::configure(
        &a,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        None,
    )
    .expect("configure metadata only; no socket is opened");
    let invitation = direct::invite(&a, &ga.group_id).expect("invite");
    let id = direct::join(&b, &gb.group_id, &invitation).expect("join");
    let joining = direct::load(&b).expect("store").relations.remove(0);
    direct::hello(
        &a,
        &id,
        &joining.invitation.as_ref().expect("invitation").secret,
        &joining.local,
        &joining.local.public_key,
    )
    .expect("record peer identity");
    direct::set_state(&a, &ga.group_id, &id, true).expect("approve");

    let channels = Channels::default();
    for (oversized, expected) in [
        (false, "Direct channel closed"),
        (true, "Direct frame exceeds its limit"),
    ] {
        let (stream, mut peer) = tokio::io::duplex(64);
        if oversized {
            peer.write_u32((MAX_FRAME + 1) as u32)
                .await
                .expect("frame header");
        }
        drop(peer);
        let result = channel(
            a.clone(),
            DispatchLocks::default(),
            channels.clone(),
            id.clone(),
            joining.local.instance_id.clone(),
            stream,
        )
        .await;
        // Reusing the same ID also proves the previous reader released its channel slot.
        assert_eq!(result, Err(expected.into()));
    }
}
