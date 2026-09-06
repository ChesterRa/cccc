use super::*;
use cccc_contracts::ActorRuntime;
use serde_json::json;

#[test]
fn public_output_status_tracks_handoff_and_retains_the_original_suppression_reason() {
    for suppressed in [false, true] {
        let f = Fixture::new();
        f.subscribe(NotificationScope::ToUser);
        let event = f.event("worker", "user", "Result", None);
        f.append(&event);
        scan(&f.home).expect("scan");
        let source = f.reference(&event);
        let view = || public_snapshot(&f.home).expect("UI view");
        assert_eq!(view()["messages"][0]["output_status"], "processing");
        reserve(&f.home, &source, "analyst").expect("reserve");
        assert_eq!(view()["messages"][0]["output_status"], "unconfirmed");
        processed(
            &f.home,
            &[source.correlation_id()],
            "analyst",
            "turn",
            "Summary",
        )
        .expect("result");
        assert_eq!(view()["messages"][0]["output_status"], "ready");
        let result = reserve_output(&f.home, "analyst:turn", "call")
            .expect("reserve output")
            .expect("result");
        assert_eq!(view()["messages"][0]["output_status"], "unconfirmed");
        assert!(prepare_output(&f.home, &result.id, "another-call").is_err());
        if suppressed {
            mark_viewed(&f.home, &[source]).expect("viewed before submission");
            assert!(
                prepare_output(&f.home, &result.id, "call")
                    .expect("prepare")
                    .is_none()
            );
            let mut prefs = preferences(&f.home).expect("preferences");
            prefs.suppress_viewed = false;
            save_preferences(&f.home, prefs).expect("later preference change");
            assert!(
                prepare_output(&f.home, &result.id, "call")
                    .expect("idempotent")
                    .is_none()
            );
            let snapshot = view();
            assert_eq!(snapshot["messages"][0]["output_status"], "suppressed");
            assert_eq!(snapshot["messages"][0]["suppression_reason"], "viewed");
            assert_eq!(snapshot["suppressed_count"], 1);
            assert_eq!(snapshot["unconfirmed_count"], 0);
        } else {
            assert!(
                prepare_output(&f.home, &result.id, "call")
                    .expect("prepare")
                    .expect("speech")
                    .ends_with("Summary")
            );
            output_submitted(&f.home, &result.id, "call").expect("browser submission receipt");
            assert_eq!(view()["messages"][0]["output_status"], "submitted");
            assert!(prepare_output(&f.home, &result.id, "call").is_err());
        }
    }
}

#[test]
fn viewed_cleanup_preserves_unscanned_requested_and_handed_off_sources() {
    for stage in 0..3 {
        let f = Fixture::new();
        f.subscribe(NotificationScope::AllChat);
        let request = f.source();
        f.append(&request);
        let reply = f.event("worker", "user", "Requested result", Some(&request.id));
        let background = f.event("worker", "peer", "Background", None);
        f.append(&reply);
        f.append(&background);
        let refs = [f.reference(&reply), f.reference(&background)];
        if stage > 0 {
            scan(&f.home).expect("scan before viewing");
        }
        if stage == 2 {
            reserve(&f.home, &refs[1], "analyst").expect("handoff before viewing");
        }
        mark_viewed(&f.home, &refs).expect("viewed");
        if stage == 0 {
            assert_eq!(snapshot(&f.home).expect("unscanned").viewed.len(), 2);
        }
        scan(&f.home).expect("scan after viewing");
        mark_viewed(&f.home, &refs).expect("duplicate observation");
        let state = snapshot(&f.home).expect("snapshot");
        let expected = if stage == 2 { 2 } else { 1 };
        assert_eq!(state.messages.len(), expected, "stage {stage}");
        assert_eq!(state.viewed.len(), expected, "no orphan at stage {stage}");
        assert!(state.viewed.contains(&refs[0]));
        assert_eq!(state.messages[0].kind, VoiceMessageKind::RequestReply);
        assert!(
            speakable_sources(&f.home, &refs.map(|r| r.correlation_id()))
                .expect("viewed sources stay silent")
                .is_empty()
        );
    }
}

#[test]
fn narrowing_scope_reclaims_viewed_candidates_but_not_unscanned_refs() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::AllChat);
    let mut prefs = preferences(&f.home).expect("preferences");
    prefs.suppress_viewed = false;
    save_preferences(&f.home, prefs).expect("allow viewed");
    let scanned = f.event("worker", "peer", "Scanned candidate", None);
    f.append(&scanned);
    scan(&f.home).expect("scan");
    let unscanned = f.event("worker", "user", "Not yet scanned", None);
    f.append(&unscanned);
    mark_viewed(&f.home, &[f.reference(&scanned), f.reference(&unscanned)]).expect("viewed");
    assert_eq!(snapshot(&f.home).expect("before narrowing").viewed.len(), 2);
    f.subscribe(NotificationScope::ToUser);
    let state = snapshot(&f.home).expect("narrowed");
    assert!(state.messages.is_empty());
    assert_eq!(state.viewed, [f.reference(&unscanned)]);
    let mut prefs = preferences(&f.home).expect("preferences");
    prefs.suppress_viewed = true;
    save_preferences(&f.home, prefs).expect("suppress viewed again");
    scan(&f.home).expect("consume unscanned source");
    let state = snapshot(&f.home).expect("after scan");
    assert!(state.messages.is_empty() && state.viewed.is_empty());
}

#[test]
fn result_completion_order_survives_reopen_duplicates_and_generation_changes() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let first_source = f.event("worker", "user", "Earlier source", None);
    let second_source = f.event("worker", "user", "Later source", None);
    f.append(&first_source);
    f.append(&second_source);
    scan(&f.home).expect("scan");
    // Completion order differs from both source arrival and random turn-ID order.
    for (source, turn, text) in [
        (f.reference(&second_source), "z-first", "Started"),
        (f.reference(&first_source), "a-second", "Finished"),
    ] {
        reserve(&f.home, &source, "z-analyst").expect("reserve");
        processed(&f.home, &[source.correlation_id()], "z-analyst", turn, text).expect("completed");
    }
    let before = snapshot(&f.home).expect("before duplicate").results;
    let next_sequence = read(&f.home)
        .expect("sequence before duplicate")
        .next_sequence;
    processed(
        &f.home,
        &[f.reference(&second_source).correlation_id()],
        "z-analyst",
        "z-first",
        "Duplicate must not replace or reorder this result",
    )
    .expect("idempotent completion");
    assert_eq!(snapshot(&f.home).expect("after duplicate").results, before);
    assert_eq!(
        read(&f.home)
            .expect("sequence after duplicate")
            .next_sequence,
        next_sequence
    );
    let next = f.event("worker", "user", "New generation", None);
    f.append(&next);
    scan(&f.home).expect("scan new result");
    let source = f.reference(&next);
    reserve(&f.home, &source, "a-restarted-analyst").expect("new Analyst");
    processed(
        &f.home,
        &[source.correlation_id()],
        "a-restarted-analyst",
        "a-new",
        "Follow-up",
    )
    .expect("new generation completion");
    let reopened = HomeLayout::from_path(f.home.root()).expect("reopen state");
    let results = snapshot(&reopened)
        .expect("catch up after Voice restarts")
        .results;
    assert_eq!(
        results.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
        ["Started", "Finished", "Follow-up"]
    );
    assert!(
        results
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    for result in results {
        reserve_output(&reopened, &result.id, "next-call")
            .expect("reserve in order")
            .expect("available");
        output_submitted(&reopened, &result.id, "next-call").expect("submitted");
    }
    assert!(
        snapshot(&reopened)
            .expect("submitted")
            .results
            .iter()
            .all(|r| r.output_submitted)
    );
}

#[test]
fn result_sequence_exhaustion_is_atomic_and_does_not_break_duplicate_completion() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let first = f.event("worker", "user", "Completed", None);
    let second = f.event("worker", "user", "Pending", None);
    f.append(&first);
    f.append(&second);
    scan(&f.home).expect("scan");
    let first_id = f.reference(&first).correlation_id();
    let second_id = f.reference(&second).correlation_id();
    for event in [&first, &second] {
        reserve(&f.home, &f.reference(event), "analyst").expect("reserve");
    }
    processed(
        &f.home,
        std::slice::from_ref(&first_id),
        "analyst",
        "first",
        "Completed",
    )
    .expect("first completion");
    let mut state = read(&f.home).expect("state");
    state.next_sequence = u64::MAX;
    let path = f.home.root().join("state/codex_voice/notifications.json");
    fs::write_secret_json(&path, &state).expect("exhausted sequence fixture");
    processed(&f.home, &[first_id], "analyst", "first", "Duplicate")
        .expect("duplicate needs no sequence");
    let before = std::fs::read(&path).expect("before failed completion");
    assert!(processed(&f.home, &[second_id], "analyst", "second", "Pending").is_err());
    assert_eq!(
        std::fs::read(&path).expect("after failed completion"),
        before
    );
    let state = snapshot(&f.home).expect("snapshot");
    assert_eq!(state.results.len(), 1);
    assert!(!state.messages[1].processed);
}

#[test]
fn output_rechecks_viewed_and_preserves_mixed_user_answers() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let mut refs = Vec::new();
    for text in ["First", "Second"] {
        let event = f.event("worker", "user", text, None);
        f.append(&event);
        refs.push(f.reference(&event));
    }
    scan(&f.home).expect("scan");
    for source in &refs {
        reserve(&f.home, source, "analyst").expect("reserve");
    }
    let ids = refs
        .iter()
        .map(VoiceMessageRef::correlation_id)
        .collect::<Vec<_>>();
    processed(&f.home, &ids, "analyst", "turn", "First and Second").expect("processed");
    let result = reserve_output(&f.home, "analyst:turn", "call")
        .expect("output")
        .expect("reserved");
    mark_viewed(&f.home, &refs[..1]).expect("viewed while queued");
    let sources = speakable_sources(&f.home, &ids).expect("recheck");
    assert_eq!(sources, refs[1..]);
    let text = output_text(&result, &sources).expect("remaining source notice");
    assert!(!text.contains("First and Second"));
    mark_viewed(&f.home, &refs[1..]).expect("all viewed");
    assert!(output_text(&result, &speakable_sources(&f.home, &ids).expect("sources")).is_none());
    let mut mixed = result.clone();
    mixed.user_answer = true;
    assert!(
        output_text(&mixed, &[])
            .expect("user answer")
            .contains("First and Second")
    );
    assert!(
        prepare_output(&f.home, &result.id, "call")
            .expect("suppressed")
            .is_none()
    );
    output_not_submitted(&f.home, &[result.id.clone()], "call").expect("close observation");
    assert!(
        reserve_output(&f.home, &result.id, "later")
            .expect("later")
            .is_none()
    );
}

#[test]
fn batched_unsent_reports_release_overflow_results_but_preserve_unknown_delivery() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let event = f.event("worker", "user", "Result", None);
    f.append(&event);
    scan(&f.home).expect("scan");
    let source = f.reference(&event);
    reserve(&f.home, &source, "analyst").expect("handoff");
    processed(
        &f.home,
        &[source.correlation_id()],
        "analyst",
        "turn",
        "Summary",
    )
    .expect("result");
    let template = snapshot(&f.home).expect("snapshot").results.remove(0);
    let ids = (0..1025)
        .map(|i| format!("analyst:turn-{i}"))
        .collect::<Vec<_>>();
    update(&f.home, |state| {
        state.results.clear();
        for (i, id) in ids
            .iter()
            .map(String::as_str)
            .chain(["unknown", "submitted"])
            .enumerate()
        {
            let mut result = template.clone();
            result.id = id.to_owned();
            result.sequence = template.sequence + i as u64;
            result.output_call = Some("call-1".into());
            result.output_submitted = id == "submitted";
            state.results.insert(id.to_owned(), result);
        }
        state.next_sequence = template.sequence + state.results.len() as u64;
        Ok(())
    })
    .expect("seed reserved results");
    assert!(
        output_not_submitted(&f.home, &ids, "call-1").is_err(),
        "oversized reports fail atomically"
    );
    output_not_submitted(&f.home, &ids[..64], "wrong-call").expect("stale reporter");
    assert!(
        snapshot(&f.home)
            .expect("still reserved")
            .results
            .iter()
            .all(|r| r.output_call.is_some())
    );
    for batch in ids.chunks(64) {
        output_not_submitted(&f.home, batch, "call-1").expect("bounded report");
    }
    output_not_submitted(&f.home, &["submitted".into()], "call-1").expect("cannot undo submission");
    let state = read(&f.home).expect("reopen persisted state");
    assert!(ids.iter().all(|id| state.results[id].output_call.is_none()));
    for id in ["unknown", "submitted"] {
        assert_eq!(state.results[id].output_call.as_deref(), Some("call-1"));
        assert!(
            reserve_output(&f.home, id, "call-2")
                .expect("no replay")
                .is_none()
        );
    }
    for id in [&ids[0], &ids[1024]] {
        assert!(
            reserve_output(&f.home, id, "call-2")
                .expect("next call")
                .is_some()
        );
    }
}

#[test]
fn output_crash_is_not_replay_but_positive_unsent_observation_is_releasable() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let event = f.event("worker", "user", "Result", None);
    f.append(&event);
    scan(&f.home).expect("scan");
    let source = f.reference(&event);
    reserve(&f.home, &source, "analyst").expect("handoff");
    processed(
        &f.home,
        &[source.correlation_id(), "user-question".into()],
        "analyst",
        "turn",
        "Answer",
    )
    .expect("mixed result");
    let result = reserve_output(&f.home, "analyst:turn", "call-1")
        .expect("output")
        .expect("reserved");
    assert!(result.user_answer);
    assert!(
        reserve_output(&f.home, &result.id, "call-2")
            .expect("restart")
            .is_none()
    );
    assert!(output_submitted(&f.home, &result.id, "call-2").is_err());
    output_not_submitted(&f.home, &[result.id.clone()], "call-1").expect("known unsent");
    assert!(
        reserve_output(&f.home, &result.id, "call-2")
            .expect("new call")
            .is_some()
    );
    output_submitted(&f.home, &result.id, "call-2").expect("browser submission");
    assert!(snapshot(&f.home).expect("snapshot").messages[0].attempted);
    assert_eq!(
        public_snapshot(&f.home).expect("UI")["unconfirmed_count"],
        0
    );
}

#[test]
fn attachment_only_is_eligible_but_bridge_copy_and_runtime_events_are_not() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::AllChat);
    let mut attachment = f.event("worker", "user", "", None);
    attachment.data.insert(
        "attachments".into(),
        json!([{"name":"report.pdf","path":"private"}]),
    );
    f.append(&attachment);
    let mut copy = f.event("worker", "user", "Source copy", None);
    copy.data.insert("dst_group_id".into(), json!("another"));
    f.append(&copy);
    let mut status = f.event("worker", "user", "Status", None);
    status.kind = "runtime.status".into();
    f.append(&status);
    scan(&f.home).expect("scan");
    assert_eq!(
        snapshot(&f.home)
            .expect("snapshot")
            .messages
            .iter()
            .map(|n| n.source.clone())
            .collect::<Vec<_>>(),
        [f.reference(&attachment)]
    );
}

#[test]
fn completed_observations_compact_but_unknown_handoffs_never_do() {
    let mut state = State {
        version: 1,
        ..Default::default()
    };
    for sequence in 0..300 {
        let source = VoiceMessageRef {
            group_id: "g_test".into(),
            event_id: sequence.to_string(),
        };
        state.notifications.insert(
            source.key(),
            VoiceNotification {
                sequence,
                source,
                kind: VoiceMessageKind::Background,
                by: "worker".into(),
                to_user: true,
                handoff: Some(VoiceMessageHandoff {
                    analyst_generation: "a".into(),
                    accepted: sequence > 0,
                }),
                processed: sequence > 0,
                attempted: sequence > 0,
            },
        );
    }
    compact(&mut state);
    assert_eq!(state.notifications.len(), 129);
    assert!(state.notifications.contains_key("g_test:0"));
}

#[test]
fn malformed_state_is_not_silently_reset() {
    let f = Fixture::new();
    let path = f.home.root().join("state/codex_voice/notifications.json");
    fs::write_secret_json(&path, &json!({"version":999})).expect("invalid fixture");
    assert!(preferences(&f.home).is_err());
    assert!(scan(&f.home).is_err());
    assert_eq!(
        fs::read_json::<serde_json::Value>(&path).expect("original retained")["version"],
        999
    );
}

#[test]
fn capacity_failure_keeps_the_cursor_and_all_pending_sources() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let path = f.home.root().join("state/codex_voice/notifications.json");
    let mut state = read(&f.home).expect("state");
    for index in 0..MAX_REFERENCES {
        state.viewed.insert(VoiceMessageRef {
            group_id: f.group.clone(),
            event_id: format!("capacity-fixture-{index}"),
        });
    }
    fs::write_secret_json(&path, &state).expect("full storage fixture");
    let before = std::fs::read(&path).expect("before");
    let event = f.event("worker", "user", "Must not be skipped", None);
    f.append(&event);
    assert_eq!(
        scan(&f.home).expect_err("capacity").kind(),
        io::ErrorKind::StorageFull
    );
    assert_eq!(std::fs::read(&path).expect("after"), before);
    // Once space is available, the same event is read, not skipped by a partial cursor commit.
    state.viewed.clear();
    fs::write_secret_json(&path, &state).expect("release fixture capacity");
    scan(&f.home).expect("retry scan");
    assert_eq!(
        snapshot(&f.home).expect("snapshot").messages[0].source,
        f.reference(&event)
    );
}

#[test]
fn switching_analyst_keeps_accepted_unfinished_sources_explicitly_unconfirmed() {
    let f = Fixture::new();
    let request = f.source();
    f.append(&request);
    let reply = f.event("worker", "user", "Progress", Some(&request.id));
    f.append(&reply);
    scan(&f.home).expect("scan");
    let source = f.reference(&reply);
    reserve(&f.home, &source, "generation").expect("reserve");
    accepted(&f.home, &source, "generation").expect("accepted");
    assert_eq!(
        public_snapshot(&f.home).expect("before")["unconfirmed_count"],
        0
    );
    register_origin(
        &f.home,
        &"b".repeat(32),
        "replacement",
        "new-thread",
        ActorRuntime::Grok,
    )
    .expect("new Analyst");
    assert_eq!(
        public_snapshot(&f.home).expect("after")["unconfirmed_count"],
        1
    );
    assert!(
        reserve(&f.home, &source, "replacement")
            .expect("no replay")
            .is_none()
    );
    let public = public_snapshot(&f.home).expect("public").to_string();
    assert!(!public.contains("Progress"));
    assert!(!public.contains(&"b".repeat(32)));
}

#[test]
fn changed_policy_and_deleted_group_are_rechecked_after_analyst_handoff() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let event = f.event("worker", "user", "Background", None);
    f.append(&event);
    scan(&f.home).expect("scan");
    let source = f.reference(&event);
    reserve(&f.home, &source, "analyst").expect("reserve");
    let ids = [source.correlation_id()];
    processed(&f.home, &ids, "analyst", "turn", "Background").expect("processed");
    f.subscribe(NotificationScope::Off);
    assert!(
        speakable_sources(&f.home, &ids)
            .expect("scope check")
            .is_empty()
    );
    assert_eq!(
        snapshot(&f.home)
            .expect("accepted source retained")
            .messages
            .len(),
        1
    );
    f.store.delete(&f.group).expect("delete fixture Group");
    assert!(
        speakable_sources(&f.home, &ids)
            .expect("source check")
            .is_empty()
    );
    scan(&f.home).expect("retire deleted Group references");
    let state = snapshot(&f.home).expect("retired");
    assert!(state.messages.is_empty() && state.results.is_empty());
    let replacement = f
        .store
        .create("Voice test", "")
        .expect("same name, new Group");
    assert!(!state.preferences.groups.contains_key(&replacement.group_id));
}

struct Fixture {
    _dir: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group: String,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp home");
        let home = HomeLayout::from_path(dir.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Voice test", "").expect("group");
        group.actors =
            vec![serde_json::from_value(json!({"id":"worker","runtime":"codex"})).expect("actor")];
        store.save(&group).expect("save group");
        Self {
            _dir: dir,
            home,
            store,
            group: group.group_id,
        }
    }
    fn event(&self, by: &str, to: &str, text: &str, reply_to: Option<&str>) -> Event {
        let mut event = Event::new("chat.message", &self.group);
        event.by = by.into();
        event.data = json!({"to":[to],"text":text,"reply_to":reply_to})
            .as_object()
            .expect("object")
            .clone();
        event
    }
    fn append(&self, event: &Event) {
        ledger::append(&self.store.ledger_path(&self.group).expect("path"), event).expect("append");
    }
    fn subscribe(&self, scope: NotificationScope) {
        let mut prefs = preferences(&self.home).expect("preferences");
        prefs.groups.insert(self.group.clone(), scope);
        save_preferences(&self.home, prefs).expect("save preferences");
    }
    fn reference(&self, event: &Event) -> VoiceMessageRef {
        VoiceMessageRef {
            group_id: self.group.clone(),
            event_id: event.id.clone(),
        }
    }
    fn source(&self) -> Event {
        let source = self.event("user", "worker", "Investigate", None);
        let token = "a".repeat(32);
        register_origin(
            &self.home,
            &token,
            "generation",
            "thread",
            ActorRuntime::Codex,
        )
        .expect("origin");
        register_request(&self.home, &token, &source).expect("request intent");
        source
    }
}

#[test]
fn defaults_and_gets_do_not_create_state_or_change_ledger() {
    let f = Fixture::new();
    assert_eq!(
        preferences(&f.home).expect("preferences"),
        VoicePreferences::default()
    );
    assert!(snapshot(&f.home).expect("snapshot").messages.is_empty());
    assert!(
        !f.home
            .root()
            .join("state/codex_voice/notifications.json")
            .exists()
    );
    assert!(
        ledger::tail(&f.store.ledger_path(&f.group).expect("path"), 10)
            .expect("tail")
            .is_empty()
    );
}

#[test]
fn save_boundaries_exclude_history_and_newly_added_categories() {
    let f = Fixture::new();
    f.append(&f.event("worker", "user", "Historical", None));
    f.subscribe(NotificationScope::ToUser);
    let old_peer = f.event("worker", "peer", "Not subscribed yet", None);
    f.append(&old_peer);
    let requested = f.event("worker", "user", "Already subscribed category", None);
    f.append(&requested);
    f.subscribe(NotificationScope::AllChat);
    let new_peer = f.event("worker", "peer", "New category", None);
    f.append(&new_peer);
    scan(&f.home).expect("scan");
    let keys = snapshot(&f.home)
        .expect("snapshot")
        .messages
        .into_iter()
        .map(|n| n.source.event_id)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys, BTreeSet::from([requested.id, new_peer.id]));
}

#[test]
fn voice_request_recipient_snapshot_matches_inbox_aliases_without_subscriptions() {
    for (selectors, expected) in [
        (vec!["@foreman"], vec!["worker"]),
        (vec!["@all"], vec!["worker", "peer"]),
        (vec!["@peers"], vec!["peer"]),
        (vec!["@all", "peer", "@foreman"], vec!["worker", "peer"]),
        (vec!["peer"], vec!["peer"]),
        (vec!["internal"], vec!["internal"]),
        (vec!["user"], vec![]),
    ] {
        let f = Fixture::new();
        let mut group = f.store.load(&f.group).expect("group");
        for value in [
            json!({"id":"peer","runtime":"codex"}),
            json!({"id":"internal","runtime":"codex","internal_kind":"voice_secretary"}),
        ] {
            group
                .actors
                .push(serde_json::from_value(value).expect("actor"));
        }
        f.store.save(&group).expect("save group");
        assert!(preferences(&f.home).expect("defaults").groups.is_empty());
        let mut source = f.source();
        source.data.insert("to".into(), json!(selectors));
        // Use a fresh event ID so only this exact recipient snapshot can match.
        source.id = Event::new("chat.message", &f.group).id;
        register_request(&f.home, &"a".repeat(32), &source).expect("request");
        f.append(&source);
        let delivered = group
            .actors
            .iter()
            .filter(|actor| crate::inbox::is_for_actor(&group, &source, &actor.id))
            .map(|actor| actor.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(delivered, expected, "inbox for {selectors:?}");
        // Role changes after sending must not change which Actors' replies the
        // Voice request accepts, and aliases must not include internal actors.
        group.actors.swap(0, 1);
        f.store.save(&group).expect("change foreman after send");
        for id in ["worker", "peer", "internal", "unrelated"] {
            f.append(&f.event(id, "user", "Reply", Some(&source.id)));
        }
        scan(&f.home).expect("scan replies");
        let notifications = snapshot(&f.home).expect("snapshot").messages;
        assert_eq!(
            notifications
                .iter()
                .map(|item| item.by.as_str())
                .collect::<Vec<_>>(),
            expected,
            "Voice replies for {selectors:?}"
        );
        assert!(
            notifications
                .iter()
                .all(|item| item.kind == VoiceMessageKind::RequestReply)
        );
    }
}

#[test]
fn ordinary_request_ack_and_final_survive_replay_without_subscription() {
    let f = Fixture::new();
    let source = f.source();
    f.append(&source);
    let foreign = f.event(
        "another-actor",
        "user",
        "Wrong respondent",
        Some(&source.id),
    );
    f.append(&foreign);
    let ack = f.event("worker", "user", "Received", Some(&source.id));
    let result = f.event("worker", "user", "Final answer", Some(&source.id));
    f.append(&ack);
    f.append(&result);
    scan(&f.home).expect("scan after all events");
    let before = snapshot(&f.home).expect("snapshot").messages;
    assert_eq!(before.len(), 2);
    assert!(
        before
            .iter()
            .all(|item| item.kind == VoiceMessageKind::RequestReply)
    );
    scan(&HomeLayout::from_path(f.home.root()).expect("reopened home"))
        .expect("recreated consumer");
    assert_eq!(snapshot(&f.home).expect("snapshot").messages, before);
}

#[test]
fn source_intent_without_committed_message_is_inert() {
    let f = Fixture::new();
    let source = f.source();
    f.append(&f.event(
        "worker",
        "user",
        "Forged reply to an uncommitted intent",
        Some(&source.id),
    ));
    scan(&f.home).expect("scan");
    assert!(snapshot(&f.home).expect("snapshot").messages.is_empty());
}

#[test]
fn burst_past_old_capacity_and_multiple_pages_is_lossless() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::AllChat);
    for n in 0..600 {
        f.append(&f.event("worker", "peer", &format!("Result {n}"), None));
    }
    for _ in 0..4 {
        scan(&f.home).expect("page");
    }
    assert_eq!(snapshot(&f.home).expect("snapshot").messages.len(), 600);
}

#[test]
fn exact_viewed_refs_suppress_background_not_requested_context_or_mail() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::AllChat);
    let source = f.source();
    f.append(&source);
    let reply = f.event("worker", "user", "Requested reply", Some(&source.id));
    let background = f.event("worker", "peer", "Mail body", None);
    f.append(&reply);
    f.append(&background);
    let refs = [f.reference(&reply), f.reference(&background)];
    mark_viewed(&f.home, &refs).expect("viewed before scan");
    mark_viewed(&f.home, &refs).expect("idempotent");
    scan(&f.home).expect("scan");
    let messages = snapshot(&f.home).expect("snapshot").messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source, refs[0]);
    assert!(
        speakable_sources(&f.home, &[refs[0].correlation_id()])
            .expect("speech check")
            .is_empty()
    );
    let events = ledger::tail(&f.store.ledger_path(&f.group).expect("path"), 100).expect("tail");
    assert_eq!(events.len(), 3);
    assert!(events.iter().all(|e| e.kind == "chat.message"));
}

#[test]
fn reserved_unknown_handoff_survives_restart_and_cannot_be_automatically_replayed() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::ToUser);
    let event = f.event("worker", "user", "Only once", None);
    f.append(&event);
    scan(&f.home).expect("scan");
    let source = f.reference(&event);
    assert!(
        reserve(&f.home, &source, "old-generation")
            .expect("reserve")
            .is_some()
    );
    // Simulate a crash before acceptance observation: a new generation must not guess.
    assert!(
        reserve(
            &HomeLayout::from_path(f.home.root()).expect("reopened home"),
            &source,
            "new-generation"
        )
        .expect("recover")
        .is_none()
    );
    assert!(accepted(&f.home, &source, "new-generation").is_err());
    assert!(
        !snapshot(&f.home).expect("snapshot").messages[0]
            .handoff
            .as_ref()
            .expect("handoff")
            .accepted
    );
}

#[test]
fn stale_revision_and_invalid_source_are_atomic_failures() {
    let f = Fixture::new();
    let stale = preferences(&f.home).expect("initial");
    f.subscribe(NotificationScope::ToUser);
    assert_eq!(
        save_preferences(&f.home, stale)
            .expect_err("conflict")
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    let before =
        std::fs::read(f.home.root().join("state/codex_voice/notifications.json")).expect("state");
    assert!(
        mark_viewed(
            &f.home,
            &[VoiceMessageRef {
                group_id: f.group.clone(),
                event_id: "missing".into()
            }]
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(f.home.root().join("state/codex_voice/notifications.json")).expect("state"),
        before
    );
}

#[test]
fn speech_keeps_host_source_names_and_detailed_material_through_preflight() {
    let f = Fixture::new();
    f.subscribe(NotificationScope::AllChat);
    let details =
        "长春 25°C，体感 26°C，17–26°C；湿度 63%，东南风 8 km/h，来源 wttr.in；尚未独立核验。";
    let mut event = f.event("worker", "user", details, None);
    event.data.insert("sender_title".into(), json!("管理员"));
    event.data.insert(
        "attachments".into(),
        json!([{"name":"weather.pdf","path":"secret-fixture-path"}]),
    );
    f.append(&event);
    let prompt =
        notification_prompt(&f.home, &event, VoiceVerbosity::Detailed).expect("Analyst input");
    let source: serde_json::Value = serde_json::from_str(
        prompt
            .split("Source JSON:\n")
            .nth(1)
            .expect("source JSON section"),
    )
    .expect("source data");
    assert_eq!(source["group_name"], "Voice test");
    assert_eq!(source["sender_name"], "管理员");
    assert_eq!(source["text"], details);
    assert!(
        prompt.contains("numbers and units") && prompt.contains("incoming Actor notifications")
    );
    assert!(!prompt.contains("secret-fixture-path"));
    scan(&f.home).expect("scan");
    let source_ref = f.reference(&event);
    reserve(&f.home, &source_ref, "analyst").expect("reserve");
    processed(
        &f.home,
        &[source_ref.correlation_id()],
        "analyst",
        "turn",
        details,
    )
    .expect("result");
    reserve_output(&f.home, "analyst:turn", "call").expect("output");
    // Removing/renaming the Actor must not replace the historical sender snapshot.
    let mut group = f.store.load(&f.group).expect("group");
    group.actors.clear();
    f.store.save(&group).expect("removed actor");
    let text = prepare_output(&f.home, "analyst:turn", "call")
        .expect("prepare")
        .expect("speech");
    assert!(text.contains("Voice test") && text.contains("管理员"));
    assert!(
        text.ends_with(details),
        "material text stays intact, not a second summary"
    );
    assert!(!text.contains("Briefly summarize"));
    mark_viewed(&f.home, &[source_ref]).expect("viewed");
    assert!(
        prepare_output(&f.home, "analyst:turn", "call")
            .expect("recheck")
            .is_none()
    );
}

#[test]
fn multiple_groups_keep_distinct_attribution_and_exclude_viewed_source_names() {
    let f = Fixture::new();
    let mut second = f.store.create("Second Group", "").expect("second group");
    second.actors = f.store.load(&f.group).expect("fixture group").actors;
    second.actors[0].title = "Shared Name".into();
    f.store.save(&second).expect("second actor");
    let mut first = f.store.load(&f.group).expect("fixture group");
    first.actors[0].title = "Shared Name".into();
    f.store.save(&first).expect("first actor");
    let mut prefs = preferences(&f.home).expect("fixture preferences");
    for id in [&f.group, &second.group_id] {
        prefs.groups.insert(id.clone(), NotificationScope::AllChat);
    }
    save_preferences(&f.home, prefs).expect("subscribe");
    let event1 = f.event("worker", "user", "First private result", None);
    let mut event2 = f.event("worker", "user", "Second result", None);
    event2.group_id = second.group_id.clone();
    for event in [&event1, &event2] {
        ledger::append(
            &f.store
                .ledger_path(&event.group_id)
                .expect("fixture ledger"),
            event,
        )
        .expect("append");
    }
    scan(&f.home).expect("scan");
    let refs = [
        VoiceMessageRef {
            group_id: f.group.clone(),
            event_id: event1.id,
        },
        VoiceMessageRef {
            group_id: second.group_id,
            event_id: event2.id,
        },
    ];
    for source in &refs {
        reserve(&f.home, source, "analyst").expect("reserve");
    }
    processed(
        &f.home,
        &refs
            .iter()
            .map(VoiceMessageRef::correlation_id)
            .collect::<Vec<_>>(),
        "analyst",
        "turn",
        "First private result and second result",
    )
    .expect("result");
    reserve_output(&f.home, "analyst:turn", "call").expect("output");
    let prepare = || {
        prepare_output(&f.home, "analyst:turn", "call")
            .expect("prepare")
            .expect("remaining notice")
    };
    let text = prepare();
    assert!(text.contains("Voice test") && text.contains("Second Group"));
    assert_eq!(text.matches("Shared Name").count(), 2);
    mark_viewed(&f.home, &refs[..1]).expect("viewed first source");
    let text = prepare();
    assert!(!text.contains("Voice test") && !text.contains("First private result"));
    assert!(text.contains("Second Group") && text.contains("Shared Name"));
}

#[test]
fn source_name_falls_back_to_actor_id_when_no_title_is_available() {
    let f = Fixture::new();
    let event = f.event("deleted-actor", "user", "Result", None);
    let prompt = notification_prompt(&f.home, &event, VoiceVerbosity::Concise).expect("prompt");
    assert!(prompt.contains("\"sender_name\":\"deleted-actor\""));
    assert!(prompt.contains("Always identify the Group and sender"));
}
