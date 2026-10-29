use super::*;
use aws_lc_rs::agreement::{ECDH_P256, PrivateKey};
use std::collections::BTreeMap;

fn home() -> (tempfile::TempDir, PushStore) {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    (temp, PushStore::new(home))
}

fn browser_key() -> String {
    let key = PrivateKey::generate(&ECDH_P256).expect("browser key");
    URL_SAFE_NO_PAD.encode(key.compute_public_key().expect("public").as_ref())
}

fn subscription(endpoint: &str, key: &str) -> NewSubscription {
    NewSubscription {
        endpoint: endpoint.into(),
        p256dh: browser_key(),
        auth: URL_SAFE_NO_PAD.encode([7_u8; 16]),
        application_server_key: key.into(),
        origin: "https://cccc.example".into(),
    }
}

fn token(user: &str) -> Owner {
    Owner::Token {
        user_id: user.into(),
        token_id: format!("{user}-token"),
        created_at: "2026-10-28T00:00:00.000000Z".into(),
    }
}

fn stored(store: &PushStore) -> Vec<Subscription> {
    store.load().expect("load").subscriptions
}

const FCM: &str = "https://fcm.googleapis.com/fcm/send/one";
const MOZILLA: &str = "https://updates.push.services.mozilla.com/wpush/v2/two";

#[test]
fn malformed_state_errors_do_not_quote_stored_values() {
    let (_temp, store) = home();
    let secret = "private-endpoint-or-key-marker";
    std::fs::write(store.path(), format!(r#"{{"subscriptions":"{secret}"}}"#))
        .expect("write invalid state");
    let error = store.public_key().expect_err("invalid type").to_string();
    assert!(error.contains("Data"), "{error}");
    assert!(error.contains("line 1"), "{error}");
    assert!(!error.contains(secret), "{error}");
}

#[test]
fn a_key_is_made_once_and_kept_owner_only() {
    let (_temp, store) = home();
    assert_eq!(store.public_key().expect("read"), None);
    let key = store.ensure_key().expect("make");
    assert_eq!(store.ensure_key().expect("again"), key);
    assert_eq!(store.public_key().expect("read"), Some(key.clone()));
    assert_eq!(URL_SAFE_NO_PAD.decode(&key).expect("base64url").len(), 65);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.path())
            .expect("file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn a_binding_needs_the_current_key_and_belongs_to_its_owner() {
    let (_temp, store) = home();
    let anyone = browser_key();
    assert!(matches!(
        store.bind(subscription(FCM, &anyone), Owner::Local, BTreeMap::new()),
        Err(BindError::NoKey)
    ));
    let key = store.ensure_key().expect("key");
    assert!(matches!(
        store.bind(subscription(FCM, &anyone), Owner::Local, BTreeMap::new()),
        Err(BindError::KeyMismatch)
    ));
    assert!(
        stored(&store).is_empty(),
        "a refused binding writes nothing"
    );

    store
        .bind(
            subscription(FCM, &format!("{key}=")),
            Owner::Local,
            BTreeMap::new(),
        )
        .expect("padded key is the same key");
    assert_eq!(
        store.owned(&Owner::Local).expect("owned"),
        [endpoint_hash(FCM)]
    );
    assert!(store.owned(&token("ana")).expect("owned").is_empty());
    assert_eq!(endpoint_hash(FCM).len(), 64);
}

#[test]
fn a_binding_with_a_damaged_origin_is_not_shown_as_enabled() {
    let (_temp, store) = home();
    let key = store.ensure_key().expect("key");
    for endpoint in [FCM, MOZILLA] {
        store
            .bind(subscription(endpoint, &key), Owner::Local, BTreeMap::new())
            .expect("bind");
    }
    let mut document = store.load().expect("load");
    for item in &mut document.subscriptions {
        if item.endpoint == FCM {
            item.origin = "file:///tmp".into();
        }
    }
    cccc_core::fs::write_secret_json(&store.path(), &document).expect("damage");
    assert_eq!(
        store.owned(&Owner::Local).expect("owned"),
        [endpoint_hash(MOZILLA)]
    );
    // Binding the same browser again repairs it.
    store
        .bind(subscription(FCM, &key), Owner::Local, BTreeMap::new())
        .expect("bind again");
    assert_eq!(store.owned(&Owner::Local).expect("owned").len(), 2);
}

fn since(event: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("g_a".to_owned(), event.to_owned())])
}

#[test]
fn every_binding_starts_where_the_ledgers_stand_and_old_sends_cannot_touch_it() {
    let (_temp, store) = home();
    let key = store.ensure_key().expect("key");
    store
        .bind(subscription(FCM, &key), token("ana"), since("e1"))
        .expect("bind");
    let first = stored(&store).remove(0);
    assert_eq!(first.since, since("e1"));

    // The same browser sent again keeps its place.
    let mut again = subscription(FCM, &key);
    again.p256dh.clone_from(&first.p256dh);
    store.bind(again, token("ana"), since("e2")).expect("again");
    let kept = stored(&store).remove(0);
    assert_eq!(
        (kept.binding.as_str(), &kept.since),
        (first.binding.as_str(), &since("e1"))
    );

    // New keys start it again, for its owner or another.
    store
        .bind(subscription(FCM, &key), token("ana"), since("e2"))
        .expect("refresh");
    let refreshed = stored(&store).remove(0);
    assert_eq!(refreshed.since, since("e2"));
    assert_ne!(refreshed.binding, first.binding);
    store
        .bind(subscription(FCM, &key), token("ben"), since("e3"))
        .expect("move");
    let moved = stored(&store).remove(0);
    assert_eq!(moved.owner, token("ben"));
    assert_eq!(moved.since, since("e3"));
    assert!(store.owned(&token("ana")).expect("owned").is_empty());

    // A send started before the move reports back with its old binding.
    store
        .forget(&HashSet::from([refreshed.binding.clone()]))
        .expect("stale forget");
    assert!(store.current(&moved.binding).expect("read").is_some());
    assert!(store.current(&refreshed.binding).expect("read").is_none());
}

#[test]
fn unbinding_removes_only_the_callers_binding_without_revealing_others() {
    let (_temp, store) = home();
    let key = store.ensure_key().expect("key");
    store
        .bind(subscription(FCM, &key), token("ana"), BTreeMap::new())
        .expect("bind");
    assert!(!store.unbind(FCM, &token("ben")).expect("other owner"));
    assert!(
        !store
            .unbind(MOZILLA, &token("ana"))
            .expect("unknown endpoint")
    );
    assert_eq!(stored(&store).len(), 1);
    assert!(store.unbind(FCM, &token("ana")).expect("owner"));
    assert!(stored(&store).is_empty());
}

#[test]
fn unreadable_state_fails_closed_and_is_left_as_it_is() {
    let (_temp, store) = home();
    let key = store.ensure_key().expect("key");
    store
        .bind(subscription(FCM, &key), Owner::Local, BTreeMap::new())
        .expect("bind");
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.path()).expect("read")).expect("json");
    document["vapid"]["pkcs8"] = "AAAA".into();
    let damaged = serde_json::to_vec(&document).expect("encode");
    std::fs::write(store.path(), &damaged).expect("write");
    assert!(store.public_key().is_err());
    assert!(store.ensure_key().is_err());
    assert!(store.owned(&Owner::Local).is_err());
    assert!(store.snapshot().is_err());
    assert!(matches!(
        store.bind(subscription(MOZILLA, &key), Owner::Local, BTreeMap::new()),
        Err(BindError::Io(_))
    ));
    assert_eq!(std::fs::read(store.path()).expect("read"), damaged);

    // Bindings without any key are as unreadable as a damaged key.
    document.as_object_mut().expect("object").remove("vapid");
    let keyless = serde_json::to_vec(&document).expect("encode");
    std::fs::write(store.path(), &keyless).expect("write");
    assert!(store.public_key().is_err());
    assert!(store.ensure_key().is_err());
    assert_eq!(std::fs::read(store.path()).expect("read"), keyless);
}

#[test]
fn an_owner_keeps_the_most_recently_bound_subscriptions() {
    let (_temp, store) = home();
    let key = store.ensure_key().expect("key");
    for index in 0..=MAX_PER_OWNER {
        store
            .bind(
                subscription(&format!("{FCM}-{index}"), &key),
                token("ana"),
                BTreeMap::new(),
            )
            .expect("bind");
    }
    store
        .bind(subscription(MOZILLA, &key), token("ben"), BTreeMap::new())
        .expect("other owner");
    let owned = store.owned(&token("ana")).expect("owned");
    assert_eq!(owned.len(), MAX_PER_OWNER);
    assert!(!owned.contains(&endpoint_hash(&format!("{FCM}-0"))));
    assert!(owned.contains(&endpoint_hash(&format!("{FCM}-{MAX_PER_OWNER}"))));
    assert_eq!(store.owned(&token("ben")).expect("owned").len(), 1);
}

#[test]
fn a_browser_just_bound_is_kept_even_when_the_others_look_newer() {
    let (_temp, store) = home();
    let key = store.ensure_key().expect("key");
    for index in 0..MAX_PER_OWNER {
        store
            .bind(
                subscription(&format!("{FCM}-{index}"), &key),
                token("ana"),
                BTreeMap::new(),
            )
            .expect("bind");
    }
    // As if they were bound before the clock was set back.
    store
        .mutate(|document| {
            for (index, item) in document.subscriptions.iter_mut().enumerate() {
                item.updated_at = format!("2099-01-01T00:00:{index:02}.000000Z");
            }
            Ok::<_, io::Error>(())
        })
        .expect("move ahead");
    store
        .bind(subscription(MOZILLA, &key), token("ana"), BTreeMap::new())
        .expect("bind now");
    let owned = store.owned(&token("ana")).expect("owned");
    assert_eq!(owned.len(), MAX_PER_OWNER);
    assert!(owned.contains(&endpoint_hash(MOZILLA)));
    assert!(!owned.contains(&endpoint_hash(&format!("{FCM}-0"))));
}
