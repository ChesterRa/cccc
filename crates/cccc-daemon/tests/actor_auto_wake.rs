#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::time::Duration;

static DAEMON_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn broadcast_delivers_to_enabled_actors_without_reenabling_disabled_recipients() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("broadcast-disabled-recipient", false).await;
    call(
        &client,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"peer2","runtime":"custom",
            "submit":"newline","command":["sh","-c","sleep 30"],"by":"user"
        }),
    )
    .await;
    call(
        &client,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer2","by":"user"}),
    )
    .await;
    call(
        &client,
        "group_settings_update",
        json!({"group_id":group_id,"by":"user","patch":{"default_send_to":"broadcast"}}),
    )
    .await;
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = cccc_core::GroupStore::new(home).expect("store");
    let ledger_path = group_ledger_path(&temp, &group_id);
    for (by, mode, to) in [
        ("peer1", "send", json!(["@all"])),
        ("user", "mail", json!(["@all"])),
        ("user", "send", json!(["@all"])),
        ("user", "send", json!([])),
    ] {
        let sent = call(
            &client,
            "send",
            json!({"group_id":group_id,"by":by,"to":to,"text":"broadcast","message_mode":mode}),
        )
        .await;
        let source_id = sent.result["event"]["id"].as_str().expect("event id");
        assert_eq!(sent.result["event"]["data"]["to"], json!(["@all"]));
        if by == "user" && mode == "send" {
            wait_for_accepted_delivery(&ledger_path, source_id).await;
        }
        let group = store.load(&group_id).expect("persisted group");
        let disabled = group
            .actors
            .iter()
            .find(|actor| actor.id == "peer2")
            .expect("peer2");
        assert!(!disabled.enabled, "{by} {mode} to={to}");
        let actors = call(
            &client,
            "actor_list",
            json!({"group_id":group_id,"by":"user"}),
        )
        .await;
        let disabled = actors.result["actors"]
            .as_array()
            .expect("actors")
            .iter()
            .find(|actor| actor["id"] == "peer2")
            .expect("peer2 status");
        assert_eq!(disabled["running"], false);
        let events = ledger::read_all(&ledger_path).expect("ledger");
        let source = events
            .iter()
            .find(|event| event.id == source_id)
            .expect("source event");
        assert!(
            cccc_core::inbox::is_for_actor(&group, source, "peer2"),
            "logical audience retained"
        );
        assert!(!events.iter().any(|event| {
            event.kind == "runtime.delivery"
                && event.data["source_event_id"] == source_id
                && event.data["actor_id"] == "peer2"
                && matches!(event.data["state"].as_str(), Some("claimed" | "accepted"))
        }));
    }
    shutdown(&client, daemon).await;
}

#[tokio::test]
async fn directed_message_restarts_an_actor_after_unexpected_process_exit() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("crash-auto-wake-test", false).await;
    call(
        &client,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"command":["sh","-c","sleep 1; exit 17"]},
            "by":"user"
        }),
    )
    .await;
    call(
        &client,
        "actor_restart",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;

    let ledger_path = group_ledger_path(&temp, &group_id);
    wait_until_async(|| async {
        let actors = raw_call(
            &client,
            "actor_list",
            json!({"group_id":group_id,"by":"user"}),
        )
        .await;
        let exit_recorded = ledger::read_all(&ledger_path).is_ok_and(|events| {
            events.iter().any(|event| {
                event.kind == "actor.stop"
                    && event.data["actor_id"] == "peer1"
                    && event.data["reason"] == "process_exit"
            })
        });
        exit_recorded
            && actors.ok
            && actors.result["actors"][0]["enabled"] == true
            && actors.result["actors"][0]["running"] == false
    })
    .await;

    call(
        &client,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "patch":{"command":["sh","-c","stty -echo; while IFS= read -r line; do printf 'RECOVERED:%s\\n' \"$line\"; done"]},
            "by":"user"
        }),
    )
    .await;
    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake after crash","message_mode":"send"}),
    )
    .await;
    let source_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("source event id")
        .to_owned();
    wait_for_accepted_delivery(&ledger_path, &source_event_id).await;
    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn a_member_that_cannot_stay_up_stops_being_restarted_and_its_messages_fail() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("start-limit-test", false).await;
    let starts = temp.path().join("starts");
    use_command(
        &client,
        &group_id,
        &format!("echo started >> {}; exit 7", starts.display()),
    )
    .await;
    let ledger_path = group_ledger_path(&temp, &group_id);

    // The second waits behind the first: both share one set of starts.
    let first = send(&client, &group_id, "first").await;
    let second = send(&client, &group_id, "second").await;
    let reason = wait_for_delivery_state(&ledger_path, &first, "failed").await;
    assert!(reason.contains("then retry"), "{reason}");
    wait_for_delivery_state(&ledger_path, &second, "failed").await;
    let used = start_count(&starts);
    assert!((1..=6).contains(&used), "{used} starts");

    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(start_count(&starts), used, "restarted after giving up");

    // A later message may find the problem gone, so it gets its own starts.
    let third = send(&client, &group_id, "third").await;
    wait_for_delivery_state(&ledger_path, &third, "failed").await;
    let later = start_count(&starts) - used;
    assert!((1..=6).contains(&later), "{later} starts");

    // A repaired member receives what failed, without anyone resending it.
    use_command(
        &client,
        &group_id,
        "stty -echo; while IFS= read -r line; do :; done",
    )
    .await;
    call(
        &client,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    for source in [&first, &second, &third] {
        wait_for_delivery_state(&ledger_path, source, "accepted").await;
    }

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn a_command_that_cannot_run_fails_its_message_instead_of_waiting_forever() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("missing-command-test", false).await;
    call(
        &client,
        "actor_update",
        json!({
            "group_id":group_id,"actor_id":"peer1",
            "patch":{"command":["/nonexistent/cccc-test-command"]},"by":"user"
        }),
    )
    .await;
    stop_peer(&client, &group_id).await;
    let source = send(&client, &group_id, "hello").await;

    let reason =
        wait_for_delivery_state(&group_ledger_path(&temp, &group_id), &source, "failed").await;
    assert!(reason.contains("then retry"), "{reason}");

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn a_short_outage_heals_while_messages_keep_arriving() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("short-outage-test", false).await;
    let ready = temp.path().join("ready");
    use_command(
        &client,
        &group_id,
        &format!(
            "[ -e {} ] || exit 1; stty -echo; while IFS= read -r line; do :; done",
            ready.display()
        ),
    )
    .await;

    let mut sources = Vec::new();
    for index in 0..6 {
        sources.push(send(&client, &group_id, &format!("message {index}")).await);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    std::fs::write(&ready, b"").expect("program returns");

    let ledger_path = group_ledger_path(&temp, &group_id);
    for source in &sources {
        wait_for_delivery_state(&ledger_path, source, "accepted").await;
    }

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn a_program_that_exits_once_while_starting_still_gets_the_message() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("second-start-test", false).await;
    let updated = temp.path().join("updated");
    use_command(
        &client,
        &group_id,
        &format!(
            "if [ ! -e {0} ]; then touch {0}; exit 0; fi; stty -echo; while IFS= read -r line; do :; done",
            updated.display()
        ),
    )
    .await;

    let source = send(&client, &group_id, "hello").await;
    wait_for_delivery_state(&group_ledger_path(&temp, &group_id), &source, "accepted").await;

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn request_reply_wakes_an_explicitly_stopped_actor() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("auto-wake-test", true).await;

    let stopped_group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(stopped_group.result["group"]["state"], "active");
    assert_eq!(stopped_group.result["group"]["actors"][0]["enabled"], false);

    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake up","message_mode":"request_reply"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "request_reply");
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    let ledger_path = group_ledger_path(&temp, &group_id);
    wait_for_accepted_delivery(&ledger_path, &sent_event_id).await;
    let actors = call(
        &client,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_wakes_an_explicitly_stopped_group() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("stopped-group-test", false).await;
    call(
        &client,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;

    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"stay stopped","message_mode":"send"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "send");
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    wait_for_accepted_delivery(&group_ledger_path(&temp, &group_id), &sent_event_id).await;
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "active");
    assert_eq!(group.result["group"]["actors"][0]["enabled"], true);
    assert_eq!(group.result["group"]["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn an_answer_waits_until_the_user_resumes_the_group_or_starts_the_requester() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    for case in ["paused", "disabled"] {
        let (temp, daemon, client, group_id) = setup(&format!("answer-{case}"), false).await;
        let ledger_path = group_ledger_path(&temp, &group_id);
        let asked = call(
            &client,
            "send",
            json!({"group_id":group_id,"by":"peer1","to":["user"],"message_mode":"request_reply","text":"",
                   "question":{"v":1,"prompt":"Which port?","options":[{"label":"8080"},{"label":"9090"}]}}),
        )
        .await;
        let asked_id = asked.result["event"]["id"].as_str().expect("asked id");
        if case == "paused" {
            call(
                &client,
                "group_set_state",
                json!({"group_id":group_id,"state":"paused","by":"user"}),
            )
            .await;
        } else {
            call(
                &client,
                "actor_update",
                json!({"group_id":group_id,"actor_id":"peer1","patch":{"enabled":false},"by":"user"}),
            )
            .await;
        }
        let answered = call(
            &client,
            "reply_request_answer",
            json!({"group_id":group_id,"by":"user","source_event_id":asked_id,"client_id":case,
                   "answer":{"v":1,"option_id":"B"}}),
        )
        .await;
        let answer_id = answered.result["event"]["id"]
            .as_str()
            .expect("answer id")
            .to_owned();

        // Answering changes neither the Group nor the Actor, so nothing is delivered.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let group = call(
            &client,
            "group_show",
            json!({"group_id":group_id,"by":"user"}),
        )
        .await;
        let expected_state = if case == "paused" { "paused" } else { "active" };
        assert_eq!(group.result["group"]["state"], expected_state, "{case}");
        assert_eq!(
            group.result["group"]["actors"][0]["enabled"],
            case == "paused",
            "{case}"
        );
        let delivered = ledger::read_all(&ledger_path).is_ok_and(|events| {
            events.iter().any(|event| {
                event.kind == "runtime.delivery" && event.data["source_event_id"] == answer_id
            })
        });
        assert!(!delivered, "{case}: delivered before the user resumed");

        // The user's own resume or start delivers the recorded answer.
        if case == "paused" {
            call(
                &client,
                "group_set_state",
                json!({"group_id":group_id,"state":"active","by":"user"}),
            )
            .await;
        } else {
            call(
                &client,
                "actor_start",
                json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
            )
            .await;
        }
        wait_for_accepted_delivery(&ledger_path, &answer_id).await;

        shutdown(&client, daemon).await;
        drop(temp);
    }
}

#[tokio::test]
async fn mail_does_not_wake_an_explicitly_stopped_group() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("stopped-group-mail-test", false).await;
    call(
        &client,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;

    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"stay in inbox","message_mode":"mail"}),
    )
    .await;
    assert_eq!(sent.result["message_mode"], "mail");
    assert_eq!(
        sent.result.get("mail_recipient_advisories"),
        Some(&json!([{"actor_id":"peer1","reasons":["group_stopped"]}]))
    );
    let sent_event_id = sent.result["event"]["id"].as_str().expect("sent event id");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let deliveries = ledger::read_all(&group_ledger_path(&temp, &group_id))
        .expect("read ledger")
        .into_iter()
        .filter(|event| {
            event.kind == "runtime.delivery" && event.data["source_event_id"] == sent_event_id
        })
        .count();
    assert_eq!(deliveries, 0);
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "stopped");
    assert_eq!(group.result["group"]["actors"][0]["running"], false);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn mail_result_names_recipients_that_cannot_act_on_it_yet() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("mail-recipient-advisories", false).await;
    // peer2 and the DeepSeek agent are enabled but have never been started.
    call(
        &client,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"peer2","runtime":"custom",
            "submit":"newline","command":["sh","-c","sleep 30"],"by":"user"
        }),
    )
    .await;
    call(
        &client,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"deep","runtime":"deepseek","by":"user"}),
    )
    .await;
    let mail = |to: Value, extra: Value| {
        let mut args = json!({
            "group_id":group_id,"by":"user","to":to,"text":"handoff","message_mode":"mail"
        });
        args.as_object_mut()
            .expect("args")
            .extend(extra.as_object().cloned().unwrap_or_default());
        args
    };
    let advisories =
        |response: &DaemonResponse| response.result.get("mail_recipient_advisories").cloned();

    let running = call(&client, "send", mail(json!(["peer1"]), json!({}))).await;
    assert_eq!(advisories(&running), None);
    assert_eq!(running.result.get("mail_advisory"), None);

    let idle = call(&client, "send", mail(json!(["peer2"]), json!({}))).await;
    assert_eq!(
        advisories(&idle),
        Some(json!([{"actor_id":"peer2","reasons":["not_running"]}]))
    );
    assert!(
        idle.result["mail_advisory"]
            .as_str()
            .is_some_and(|note| note.contains("Writing Mail does not start an Actor"))
    );
    // DeepSeek runs in a process CCCC supervises, so its state is reported too.
    let deep = call(&client, "send", mail(json!(["deep"]), json!({}))).await;
    assert_eq!(
        advisories(&deep),
        Some(json!([{"actor_id":"deep","reasons":["not_running"]}]))
    );
    let both = call(&client, "send", mail(json!(["peer1", "peer2"]), json!({}))).await;
    assert_eq!(
        advisories(&both),
        Some(json!([{"actor_id":"peer2","reasons":["not_running"]}]))
    );
    // A broadcast names nobody, so it reports nobody.
    let broadcast = call(&client, "send", mail(json!(["@all"]), json!({}))).await;
    assert_eq!(advisories(&broadcast), None);

    // Mail sent as a reply reports the same way.
    let question = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"peer1","to":["user"],"text":"who takes this?","message_mode":"send"}),
    )
    .await;
    let reply = call(
        &client,
        "reply",
        json!({
            "group_id":group_id,"by":"user","reply_to":question.result["event"]["id"],
            "to":["peer2"],"text":"peer2 does","message_mode":"mail"
        }),
    )
    .await;
    assert_eq!(
        advisories(&reply),
        Some(json!([{"actor_id":"peer2","reasons":["not_running"]}]))
    );

    // A replayed request returns its original event without a fresh check.
    let first = call(
        &client,
        "send",
        mail(json!(["peer2"]), json!({"client_id":"mail-1"})),
    )
    .await;
    assert!(advisories(&first).is_some());
    let replay = call(
        &client,
        "send",
        mail(json!(["peer2"]), json!({"client_id":"mail-1"})),
    )
    .await;
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(replay.result["event"]["id"], first.result["event"]["id"]);
    assert_eq!(advisories(&replay), None);

    call(
        &client,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer2","by":"user"}),
    )
    .await;
    let disabled = call(&client, "send", mail(json!(["peer2"]), json!({}))).await;
    assert_eq!(
        advisories(&disabled),
        Some(json!([{"actor_id":"peer2","reasons":["disabled"]}]))
    );

    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    let paused = call(&client, "send", mail(json!(["peer1", "peer2"]), json!({}))).await;
    assert_eq!(
        advisories(&paused),
        Some(json!([
            {"actor_id":"peer1","reasons":["group_paused"]},
            {"actor_id":"peer2","reasons":["disabled","group_paused"]}
        ]))
    );

    // Reporting changes nothing: no delivery, and the Group stays paused.
    let events = ledger::read_all(&group_ledger_path(&temp, &group_id)).expect("ledger");
    assert!(
        !events
            .iter()
            .any(|event| { event.kind == "runtime.delivery" && event.data["actor_id"] == "peer2" })
    );
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "paused");

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_resumes_a_paused_group_and_delivers() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("paused-session-recovery", false).await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"resume-on-send","message_mode":"send"}),
    )
    .await;
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    wait_for_accepted_delivery(&group_ledger_path(&temp, &group_id), &sent_event_id).await;
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "active");
    assert_eq!(group.result["group"]["actors"][0]["enabled"], true);
    assert_eq!(group.result["group"]["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

#[tokio::test]
async fn directed_message_resumes_a_paused_group_and_stopped_actor() {
    let _guard = DAEMON_TEST_LOCK.lock().await;
    let (temp, daemon, client, group_id) = setup("actor-start-recovery", false).await;
    call(
        &client,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    )
    .await;
    call(
        &client,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    let sent = call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"message-A","message_mode":"send"}),
    )
    .await;
    let sent_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("sent event id")
        .to_owned();
    wait_for_accepted_delivery(&group_ledger_path(&temp, &group_id), &sent_event_id).await;
    let group = call(
        &client,
        "group_show",
        json!({"group_id":group_id,"by":"user"}),
    )
    .await;
    assert_eq!(group.result["group"]["state"], "active");
    assert_eq!(group.result["group"]["actors"][0]["enabled"], true);
    assert_eq!(group.result["group"]["actors"][0]["running"], true);

    shutdown(&client, daemon).await;
    drop(temp);
}

async fn setup(
    title: &str,
    reads_delivery: bool,
) -> (
    tempfile::TempDir,
    tokio::task::JoinHandle<anyhow::Result<()>>,
    DaemonClient,
    String,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let daemon = tokio::spawn(cccc_daemon::run(home.clone()));
    wait_until(|| cccc_daemon::DaemonPaths::new(home.clone()).address.exists()).await;
    let client = DaemonClient::new(home.clone());
    let created = call(&client, "group_create", json!({"title":title,"by":"user"})).await;
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    call(
        &client,
        "attach",
        json!({"group_id":group_id,"path":workspace,"by":"user"}),
    )
    .await;
    let command = if reads_delivery {
        "stty -echo; IFS= read -r preamble; IFS= read -r message; printf 'PREAMBLE:%s\\nMESSAGE:%s' \"$preamble\" \"$message\"; sleep 2"
    } else {
        "sleep 30"
    };
    call(
        &client,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"custom",
            "submit":"newline",
            "command":["sh","-c",command],
            "by":"user"
        }),
    )
    .await;
    call(
        &client,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    if reads_delivery {
        call(
            &client,
            "actor_stop",
            json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
        )
        .await;
    }
    (temp, daemon, client, group_id)
}

async fn shutdown(client: &DaemonClient, daemon: tokio::task::JoinHandle<anyhow::Result<()>>) {
    call(client, "shutdown", json!({})).await;
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon shutdown timeout")
        .expect("daemon task")
        .expect("daemon result");
}

async fn call(client: &DaemonClient, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(client, op, args).await;
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

async fn raw_call(client: &DaemonClient, op: &str, args: Value) -> DaemonResponse {
    client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        })
        .await
        .expect("daemon request")
}

fn group_ledger_path(temp: &tempfile::TempDir, group_id: &str) -> std::path::PathBuf {
    temp.path()
        .join("rust-home/groups")
        .join(group_id)
        .join("ledger.jsonl")
}

async fn stop_peer(client: &DaemonClient, group_id: &str) {
    call(
        client,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
}

/// Leave peer1 not running, with `script` as what a start will run.
async fn use_command(client: &DaemonClient, group_id: &str, script: &str) {
    stop_peer(client, group_id).await;
    call(
        client,
        "actor_update",
        json!({
            "group_id":group_id,"actor_id":"peer1",
            "patch":{"command":["sh","-c",script]},"by":"user"
        }),
    )
    .await;
}

async fn send(client: &DaemonClient, group_id: &str, text: &str) -> String {
    let sent = call(
        client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":text,"message_mode":"send"}),
    )
    .await;
    sent.result["event"]["id"]
        .as_str()
        .expect("source event id")
        .to_owned()
}

fn start_count(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

/// Waits out a wake's terminal writes; returns the state's reason.
async fn wait_for_delivery_state(
    ledger_path: &std::path::Path,
    source_event_id: &str,
    state: &str,
) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let latest = ledger::read_all(ledger_path)
            .expect("ledger")
            .into_iter()
            .rev()
            .find(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == source_event_id
                    && event.data["actor_id"] == "peer1"
            });
        if let Some(event) = latest.as_ref().filter(|event| event.data["state"] == state) {
            return event.data["reason"].as_str().unwrap_or_default().to_owned();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "delivery never reached {state}: {:?}",
            latest.map(|event| event.data)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_for_accepted_delivery(ledger_path: &std::path::Path, source_event_id: &str) {
    wait_until_async(|| async {
        ledger::read_all(ledger_path).is_ok_and(|events| {
            events.iter().any(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == source_event_id
                    && event.data["actor_id"] == "peer1"
                    && event.data["state"] == "accepted"
            })
        })
    })
    .await;
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until_async<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
