//! Local protocol fixture for Grok's durable send_now / queued_after_cancel order.
use super::super::acp::{AcpClient, PermissionPolicy, PromptCompletion};
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct FixtureProcess(Child);
impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn update(kind: &str, text: &str, prompt: Option<&str>) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":"fixture-session", "_meta":{"promptId":prompt},
        "update":{"sessionUpdate":kind,"content":{"type":"text","text":text}}
    }})
}
fn completed(prompt: &str, reason: &str) -> Value {
    json!({"jsonrpc":"2.0","method":"_x.ai/session/update","params":{
        "sessionId":"fixture-session",
        "update":{"sessionUpdate":"turn_completed","prompt_id":prompt,"stop_reason":reason}
    }})
}
fn emit(message: Value) -> String {
    // Fixture strings are static, but quote JSON as shell data nevertheless.
    format!(
        "printf '%s\\n' '{}'\n",
        message.to_string().replace('\'', "'\\''")
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_redirect_keeps_source_on_new_turn_despite_late_rpc_and_duplicate_completion() {
    for rpc_position in 0..3 {
        let old_response = json!({"jsonrpc":"2.0","id":2,"result":{"stopReason":"cancelled"}});
        let mut redirect = String::new();
        if rpc_position == 0 {
            redirect += &emit(old_response.clone());
        }
        redirect += &emit(completed("provider-old", "cancelled"));
        // A replay of the same native input must not consume its reservation.
        let mut replay = update("user_message_chunk", "weather-source", None);
        replay["params"]["_meta"]["isReplay"] = json!(true);
        redirect += &emit(replay);
        redirect += &emit(update("user_message_chunk", "weather-source", None));
        if rpc_position == 1 {
            redirect += &emit(old_response.clone());
        }
        redirect += &emit(update("agent_thought_chunk", "", Some("provider-new")));
        // Duplicate terminals and stale text must not settle or contaminate new work.
        redirect += &emit(completed("provider-old", "cancelled"));
        redirect += &emit(update(
            "agent_message_chunk",
            "stale old output",
            Some("provider-old"),
        ));
        if rpc_position == 2 {
            redirect += &emit(old_response);
        }
        redirect += "sleep 0.35\n";
        redirect += &emit(update(
            "agent_message_chunk",
            "Changchun: 25 C, range 17-26 C",
            Some("provider-new"),
        ));
        redirect += &emit(completed("provider-new", "end_turn"));
        redirect += &emit(completed("provider-new", "end_turn"));
        redirect += &emit(json!({"jsonrpc":"2.0","id":3,"result":{}}));
        let script = format!(
            r#"while IFS= read -r line; do
case "$line" in
*'"method":"session/new"'*) {} ;;
*'"method":"session/prompt"'*) {} {} ;;
*'"method":"fixture/redirect"'*) {} ;;
esac
done"#,
            emit(json!({"jsonrpc":"2.0","id":1,"result":{"sessionId":"fixture-session"}})),
            emit(update("user_message_chunk", "wait for weather", None)),
            emit(update(
                "agent_message_chunk",
                "Waiting",
                Some("provider-old")
            )),
            redirect,
        );
        let mut process = FixtureProcess(
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("fixture"),
        );
        let client = AcpClient::new(
            process.0.stdin.take().expect("fixture stdin"),
            process.0.stdout.take().expect("fixture stdout"),
            "fixture-generation".into(),
            "grok",
            PermissionPolicy::Reject,
            PromptCompletion::BoundedPostResponseDrain,
        )
        .expect("client");
        let mut events = client.subscribe();
        client
            .request("session/new", json!({}), Duration::from_secs(2))
            .await
            .expect("session");
        let old = client
            .start_prompt("fixture-session", "user-request", "wait for weather")
            .await
            .expect("initial prompt");
        client
            .register_native_input("voice-result:weather", "weather-source")
            .await
            .expect("source");
        client
            .request("fixture/redirect", json!({}), Duration::from_secs(2))
            .await
            .expect("redirect");
        let observed = std::iter::from_fn(|| events.try_recv().ok()).collect::<Vec<_>>();
        let starts = observed
            .iter()
            .filter(|e| e.requested_delegation_id.as_deref() == Some("voice-result:weather"))
            .collect::<Vec<_>>();
        assert_eq!(starts.len(), 1, "one non-replayed source admission");
        let new = &starts[0].message["params"]["turn"]["id"];
        assert_ne!(new, &json!(old));
        assert!(
            new.as_str().is_some(),
            "source starts a new turn, not an attachment to the old turn"
        );
        let terminals = observed
            .iter()
            .filter(|e| e.message["method"] == "turn/completed")
            .collect::<Vec<_>>();
        assert_eq!(
            terminals.len(),
            2,
            "no duplicate completion, rpc position {rpc_position}"
        );
        assert_eq!(terminals[0].message["params"]["turn"]["id"], old);
        assert_eq!(
            terminals[0].message["params"]["turn"]["status"],
            "cancelled"
        );
        assert_eq!(&terminals[1].message["params"]["turn"]["id"], new);
        assert_eq!(
            terminals[1].message["params"]["turn"]["status"],
            "completed"
        );
        let text = observed
            .iter()
            .filter(|e| &e.message["params"]["turnId"] == new)
            .filter_map(|e| e.message["params"]["delta"].as_str())
            .collect::<String>();
        assert_eq!(text, "Changchun: 25 C, range 17-26 C");
        client.close().await;
    }
}
