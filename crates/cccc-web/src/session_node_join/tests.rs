use super::*;

fn home() -> (tempfile::TempDir, cccc_core::HomeLayout) {
    let temp = tempfile::tempdir().expect("private Home");
    let home = cccc_core::HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("initialize");
    (temp, home)
}
fn input(code: u32) -> CreateRequest {
    CreateRequest {
        node_id: iroh::SecretKey::from_bytes(&[1; 32]).public().to_string(),
        name: "Laptop".into(),
        device_code: format!("{code:064x}"),
    }
}
fn poll(id: &str, code: u32) -> PollRequest {
    PollRequest {
        request_id: id.into(),
        device_code: format!("{code:064x}"),
    }
}

#[test]
fn retries_are_owned_by_the_secret_and_approval_is_not_polling() {
    let (_temp, home) = home();
    let mut queue = Queue::default();
    let first = queue.create(input(1), &home).expect("request");
    assert_eq!(
        queue
            .create(input(1), &home)
            .expect("lost reply retry")
            .request_id,
        first.request_id
    );
    let another = queue
        .create(input(2), &home)
        .expect("new flow on same public device ID");
    assert_ne!(first.request_id, another.request_id);
    assert_eq!(
        queue
            .poll(poll(&first.request_id, 1), &home)
            .expect("poll")
            .status,
        JoinStatus::Pending
    );
    assert!(queue.poll(poll(&first.request_id, 2), &home).is_err());
    assert_eq!(queue.pending(&home).expect("pending").len(), 2);
    assert!(
        serde_json::to_value(&first)
            .expect("response")
            .get("device_code")
            .is_none()
    );
    queue
        .approve(&first.request_id, "synthetic-ticket".into(), &home)
        .expect("explicit confirmation");
    assert!(
        queue
            .poll(poll(&first.request_id, 1), &home)
            .expect("claim")
            .ticket
            .as_deref()
            == Some("synthetic-ticket")
    );
    assert!(
        queue
            .poll(poll(&first.request_id, 1), &home)
            .expect("lost claim reply")
            .ticket
            .as_deref()
            == Some("synthetic-ticket")
    );
    queue
        .deny(&another.request_id, &home)
        .expect("explicit denial");
    assert_eq!(
        queue
            .poll(poll(&another.request_id, 2), &home)
            .expect("denied")
            .status,
        JoinStatus::Denied
    );
    assert!(queue.approval(&another.request_id, &home).is_err());
}

#[test]
fn unauthenticated_queue_is_rate_bounded_and_expiry_or_restart_cannot_approve() {
    let (_temp, home) = home();
    let mut queue = Queue::default();
    let first = queue.create(input(0), &home).expect("first");
    for code in 1..16 {
        queue.create(input(code), &home).expect("bounded burst");
    }
    assert!(queue.create(input(16), &home).is_err());
    assert_eq!(queue.pending(&home).expect("bounded entries").len(), 16);
    for entry in queue.entries.values_mut() {
        entry.deadline = Instant::now();
    }
    assert_eq!(
        queue
            .poll(poll(&first.request_id, 0), &home)
            .expect("expired")
            .status,
        JoinStatus::Expired
    );
    assert!(queue.approval(&first.request_id, &home).is_err());
    assert_eq!(
        Queue::default()
            .poll(poll(&first.request_id, 0), &home)
            .expect("restart")
            .status,
        JoinStatus::Expired
    );
}
