use super::*;
use serde_json::json;

struct Fixture {
    temp: tempfile::TempDir,
    home: HomeLayout,
    group: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        Self {
            temp,
            home,
            group: format!("g_{}", uuid::Uuid::new_v4().simple()),
        }
    }
    async fn start(&self, script: &str, mut extra: Value) -> Value {
        extra["group_id"] = json!(self.group);
        extra["command"] = json!(["sh", "-c", script]);
        start(
            &self.home,
            self.temp.path(),
            extra.as_object().expect("fixture operation"),
        )
        .await
        .expect("start")
    }
    async fn poll(&self, id: &str, mut extra: Value) -> Value {
        extra["group_id"] = json!(self.group);
        extra["session_id"] = json!(id);
        write(&self.home, extra.as_object().expect("fixture operation"))
            .await
            .expect("poll")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        shutdown(&self.home).expect("cleanup");
    }
}

async fn wait_for_command_exit(command: &CommandSession) -> cccc_runtime::SessionStatus {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let command = command.clone();
            let status = blocking(move || command.status().map_err(|error| error.to_string()))
                .await
                .expect("fixture command status");
            if !status.running {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture command did not exit within 10 seconds")
}

#[tokio::test]
async fn waits_for_output_and_does_not_replay_consumed_bytes() {
    let f = Fixture::new();
    let result = f
        .start(
            "sleep 0.12; printf first; sleep 0.2; printf second; sleep 0.2",
            json!({}),
        )
        .await;
    assert_eq!(result["output"], "first");
    let id = result["session_id"].as_str().expect("fixture operation");
    let empty = f.poll(id, json!({"yield_time_ms":0})).await;
    assert_eq!(empty["output"], "");
    let result = f.poll(id, json!({})).await;
    assert_eq!(result["output"], "second");
    let ended = f.poll(id, json!({})).await;
    assert_eq!(ended["output"], "");
    assert_eq!(ended["closed"], true);
    assert_eq!(ended["status"]["exit_code"], 0);
}

#[tokio::test]
async fn budgets_and_pages_finished_output_without_losing_the_tail() {
    let f = Fixture::new();
    let started = f
        .start(
            "printf 0123456789ABCDEFGHIJ",
            json!({"yield_time_ms":0,"max_output_bytes":4}),
        )
        .await;
    let id = started["session_id"]
        .as_str()
        .expect("fixture operation")
        .to_owned();
    let mut output = started["output"]
        .as_str()
        .expect("fixture operation")
        .to_owned();
    let mut result = started;
    while result["closed"] != true {
        result = f.poll(&id, json!({"max_output_bytes":4})).await;
        let text = result["output"].as_str().expect("fixture operation");
        assert!(text.len() <= 4);
        output.push_str(text);
    }
    assert_eq!(output, "0123456789ABCDEFGHIJ");
    assert_eq!(result["status"]["exit_code"], 0);
    assert!(
        session(
            &f.home,
            json!({"session_id":id})
                .as_object()
                .expect("fixture operation")
        )
        .is_err()
    );
}

#[tokio::test]
async fn timeout_runs_without_polling_and_preserves_output() {
    let f = Fixture::new();
    let result = f
        .start(
            "printf before-timeout; sleep 30",
            json!({"timeout_s":1,"yield_time_ms":0}),
        )
        .await;
    let id = result["session_id"].as_str().expect("fixture operation");
    let (_, owner) = session(
        &f.home,
        json!({"session_id":id})
            .as_object()
            .expect("fixture operation"),
    )
    .expect("fixture operation");
    wait_for_command_exit(&owner.command).await;
    let final_output = f.poll(id, json!({})).await;
    assert_eq!(
        format!(
            "{}{}",
            result["output"].as_str().expect("fixture operation"),
            final_output["output"].as_str().expect("fixture operation")
        ),
        "before-timeout"
    );
    assert_eq!(final_output["timed_out"], true);
    assert_eq!(final_output["closed"], true);
}

#[tokio::test]
async fn termination_returns_unread_output_and_keeps_a_natural_exit_code() {
    let f = Fixture::new();
    let result = f
        .start(
            "printf final-marker; sleep 0.1; exit 7",
            json!({"yield_time_ms":0}),
        )
        .await;
    let id = result["session_id"].as_str().expect("fixture operation");
    tokio::time::sleep(Duration::from_millis(250)).await;
    let ended = f
        .poll(id, json!({"terminate":true,"chars":"ignored"}))
        .await;
    assert_eq!(ended["status"]["exit_code"], 7);
    assert_eq!(ended["timed_out"], false);
    assert_eq!(
        format!(
            "{}{}",
            result["output"].as_str().expect("fixture operation"),
            ended["output"].as_str().expect("fixture operation")
        ),
        "final-marker"
    );
}

#[tokio::test]
async fn polling_waits_without_blocking_other_sessions_and_stdin_works() {
    let f = Fixture::new();
    let waiting = f
        .start(
            "read value; printf 'answer=%s' \"$value\"",
            json!({"yield_time_ms":0}),
        )
        .await;
    let id = waiting["session_id"].as_str().expect("fixture operation");
    let begin = Instant::now();
    let (quiet, quick) = tokio::join!(
        f.poll(id, json!({"yield_time_ms":180})),
        f.start("printf independent", json!({}))
    );
    assert!(begin.elapsed() >= Duration::from_millis(150));
    assert_eq!(quiet["output"], "");
    assert_eq!(quick["output"], "independent");
    let mut result = f.poll(id, json!({"chars":"ok\n"})).await;
    let mut output = result["output"]
        .as_str()
        .expect("fixture operation")
        .to_owned();
    while result["closed"] != true {
        result = f.poll(id, json!({})).await;
        output.push_str(result["output"].as_str().expect("fixture operation"));
    }
    assert!(output.contains("answer=ok"), "{output}");
}

#[tokio::test]
async fn ownership_shutdown_and_reaping_do_not_touch_other_sessions() {
    let f = Fixture::new();
    let other = Fixture::new();
    let first = f.start("sleep 30", json!({"yield_time_ms":0})).await;
    let second = other.start("sleep 30", json!({"yield_time_ms":0})).await;
    let id = first["session_id"].as_str().expect("fixture operation");
    let (_, first_owner) = session(
        &f.home,
        json!({"session_id":id})
            .as_object()
            .expect("fixture operation"),
    )
    .expect("fixture operation");
    let (_, other_owner) = session(
        &other.home,
        json!({"session_id":second["session_id"]})
            .as_object()
            .expect("fixture operation"),
    )
    .expect("fixture operation");
    assert!(
        write(
            &other.home,
            json!({"session_id":id})
                .as_object()
                .expect("fixture operation")
        )
        .await
        .is_err()
    );
    assert!(
        write(
            &f.home,
            json!({"session_id":id,"group_id":"wrong"})
                .as_object()
                .expect("fixture operation")
        )
        .await
        .is_err()
    );
    // Local commands must not join the Actor registry or its global reaper.
    assert!(cccc_runtime::status(&f.group, id).is_err());
    crate::shutdown(&f.home).await;
    assert!(
        !first_owner
            .command
            .status()
            .expect("fixture operation")
            .running
    );
    assert!(
        other_owner
            .command
            .status()
            .expect("fixture operation")
            .running
    );
}

#[tokio::test]
async fn cwd_environment_and_invalid_arguments_are_handled_before_launch() {
    let f = Fixture::new();
    std::fs::create_dir(f.temp.path().join("subdir")).expect("fixture operation");
    let result = f
        .start(
            "printf '%s:%s' \"${PWD##*/}\" \"$CCCC_TOOL_CONTRACT_VALUE\"",
            json!({"workdir":"subdir","env":{"CCCC_TOOL_CONTRACT_VALUE":"fixture"}}),
        )
        .await;
    assert_eq!(result["output"], "subdir:fixture");
    for args in [
        json!({"yield_time_ms":-1}),
        json!({"timeout_s":0}),
        json!({"max_output_bytes":"8"}),
    ] {
        assert!(
            start(
                &f.home,
                f.temp.path(),
                args.as_object().expect("fixture operation")
            )
            .await
            .is_err()
        );
    }
}

#[tokio::test]
async fn concurrent_polls_consume_each_output_byte_once() {
    let f = Fixture::new();
    let start = f
        .start(
            "sleep 0.1; printf abcdefgh; sleep 1",
            json!({"yield_time_ms":0}),
        )
        .await;
    let id = start["session_id"].as_str().expect("fixture operation");
    let (a, b) = tokio::join!(
        f.poll(id, json!({"max_output_bytes":4})),
        f.poll(id, json!({"max_output_bytes":4}))
    );
    assert_eq!(
        format!(
            "{}{}",
            a["output"].as_str().expect("fixture operation"),
            b["output"].as_str().expect("fixture operation")
        ),
        "abcdefgh"
    );
}

#[tokio::test]
async fn unicode_paging_and_incomplete_final_utf8_make_forward_progress() {
    let f = Fixture::new();
    let mut result = f
        .start(
            "printf '你好abc'; printf '\\342'",
            json!({"yield_time_ms":0,"max_output_bytes":1}),
        )
        .await;
    let id = result["session_id"]
        .as_str()
        .expect("fixture operation")
        .to_owned();
    let mut output = result["output"]
        .as_str()
        .expect("fixture operation")
        .to_owned();
    for _ in 0..12 {
        if result["closed"] == true {
            break;
        }
        result = f.poll(&id, json!({"max_output_bytes":1})).await;
        assert!(result["output"].as_str().expect("fixture operation").len() <= 4);
        output.push_str(result["output"].as_str().expect("fixture operation"));
    }
    assert_eq!(output, "你好abc?");
    assert_eq!(result["closed"], true);
}

#[tokio::test]
async fn termination_interrupts_a_pending_long_poll() {
    let f = Fixture::new();
    let result = f.start("sleep 30", json!({"yield_time_ms":0})).await;
    let id = result["session_id"].as_str().expect("fixture operation");
    let begin = Instant::now();
    let (_, stopped) = tokio::join!(f.poll(id, json!({"yield_time_ms":30000})), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        f.poll(id, json!({"terminate":true})).await
    });
    assert!(begin.elapsed() < Duration::from_secs(2));
    assert_eq!(stopped["closed"], true);
}

#[tokio::test]
async fn dropped_waits_keep_deadline_cleanup() {
    let f = Fixture::new();
    let args = json!({"group_id":f.group,"command":["sh","-c","sleep 30"],"timeout_s":1,"yield_time_ms":30000});
    assert!(
        tokio::time::timeout(
            Duration::from_millis(80),
            start(
                &f.home,
                f.temp.path(),
                args.as_object().expect("fixture operation")
            )
        )
        .await
        .is_err()
    );
    let abandoned = sessions()
        .lock()
        .expect("fixture operation")
        .values()
        .find(|s| s.home == f.home.root())
        .cloned()
        .expect("fixture operation");
    wait_for_command_exit(&abandoned.command).await;
    assert!(abandoned.timed_out.load(Ordering::Acquire));
}

#[tokio::test]
async fn large_output_reports_expiration_without_consuming_the_cursor() {
    let f = Fixture::new();
    let initial = f
        .start(
            "head -c 2200000 /dev/zero | tr '\\000' x",
            json!({"yield_time_ms":0,"max_output_bytes":1}),
        )
        .await;
    let id = initial["session_id"].as_str().expect("fixture operation");
    let (_, owner) = session(
        &f.home,
        json!({"session_id":id})
            .as_object()
            .expect("fixture operation"),
    )
    .expect("fixture operation");
    // Elapsed time does not prove the PTY reader has filled the bounded buffer.
    // Status drains completed output without advancing the MCP consumption cursor.
    let status = wait_for_command_exit(&owner.command).await;
    assert_eq!(status.exit_code, Some(0));
    assert_eq!(
        owner
            .command
            .history_since(2_200_000, 1)
            .expect("fixture history")
            .end_cursor,
        2_200_000
    );
    assert_eq!(
        *owner.cursor.lock().expect("fixture cursor"),
        initial["cursor"].as_u64().expect("initial cursor")
    );
    let page = f.poll(id, json!({"max_output_bytes":1000})).await;
    assert_eq!(page["cursor_expired"], true);
    assert_eq!(page["output"], "x".repeat(1000));
    assert_eq!(page["has_more"], true);
}

#[tokio::test]
async fn timeout_breaks_backpressured_input_and_stops_descendants() {
    let f = Fixture::new();
    let started = f
        .start(
            "stty -icanon -echo; (sleep 2; printf leaked > child-leak) & printf ready; sleep 30",
            json!({"timeout_s":1}),
        )
        .await;
    let id = started["session_id"].as_str().expect("fixture operation");
    let args = json!({"session_id":id,"chars":"x".repeat(2_000_000)});
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        write(&f.home, args.as_object().expect("fixture operation")),
    )
    .await;
    assert!(
        result.is_ok(),
        "backpressured input prevented deadline cleanup"
    );
    let ended = f.poll(id, json!({})).await;
    assert_eq!(ended["timed_out"], true);
    assert_eq!(ended["status"]["running"], false);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(!f.temp.path().join("child-leak").exists());
}

#[tokio::test]
async fn session_output_and_stdin_remain_owned_by_the_original_actor_and_binding() {
    let f = Fixture::new();
    let started = f
        .start(
            "read value; printf '%s' \"$value\"",
            json!({"yield_time_ms":0,"by":"alpha","_cccc_web_binding":{"revision":"r1"}}),
        )
        .await;
    let id = started["session_id"].as_str().expect("session");
    for (actor, revision) in [("beta", "r1"), ("alpha", "r2")] {
        let args = json!({"group_id":f.group,"session_id":id,"by":actor,"_cccc_web_binding":{"revision":revision},"chars":"stolen\n"});
        assert!(
            write(&f.home, args.as_object().expect("valid test fixture"))
                .await
                .is_err()
        );
    }
    let output = f
        .poll(
            id,
            json!({"by":"alpha","_cccc_web_binding":{"revision":"r1"},"chars":"owned\n"}),
        )
        .await;
    assert!(
        output["output"]
            .as_str()
            .expect("valid test fixture")
            .contains("owned")
    );
    assert!(
        !output["output"]
            .as_str()
            .expect("valid test fixture")
            .contains("stolen")
    );
}
