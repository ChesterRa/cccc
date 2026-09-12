//! Provider boundary is simulated; the real client poll/event loop follows moves.
use super::{ClaudeClient, MANAGED_AGENT_DISCONNECTED_METHOD};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub(super) async fn verify_round_trip(client: &ClaudeClient, original: &Path, state: &Path) {
    let worktree = original
        .parent()
        .expect("transcript parent directory")
        .with_file_name("worktree");
    std::fs::create_dir_all(&worktree).expect("transcript relocation fixture operation");
    let relocated = worktree.join(original.file_name().expect("transcript file name"));
    let mut events = client.subscribe();
    for (index, (source, target)) in [
        (original, relocated.as_path()),
        (relocated.as_path(), original),
    ]
    .into_iter()
    .enumerate()
    {
        let text = format!("after worktree move {index}");
        client
            .register_native_input(&format!("move-{index}"), &text)
            .await
            .expect("transcript relocation fixture operation");
        // Leave the old published pointer in place, as Agent View does.
        std::fs::copy(source, target).expect("transcript relocation fixture operation");
        std::fs::remove_file(source).expect("transcript relocation fixture operation");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(target)
            .expect("transcript relocation fixture operation");
        for record in [
            json!({"type":"user","sessionId":"52b41c61-e23c-4b7c-8b60-809c347451b5","promptId":format!("move-prompt-{index}"),"message":{"content":text}}),
            json!({"type":"assistant","sessionId":"52b41c61-e23c-4b7c-8b60-809c347451b5","message":{"content":[{"type":"text","text":text}]}}),
            json!({"type":"system","sessionId":"52b41c61-e23c-4b7c-8b60-809c347451b5","subtype":"turn_duration"}),
        ] {
            writeln!(file, "{record}").expect("transcript relocation fixture operation");
        }
        let mut answers = 0;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = events.recv().await.expect("transcript event");
                let method = event.message["method"].as_str().unwrap_or_default();
                assert_ne!(method, MANAGED_AGENT_DISCONNECTED_METHOD, "{event:?}");
                if method == "item/completed"
                    && event.message["params"]["item"]["type"] == "agentMessage"
                {
                    assert_eq!(event.message["params"]["item"]["text"], text);
                    answers += 1;
                }
                if method == "turn/completed" {
                    break;
                }
            }
        })
        .await
        .expect("delivery after relocation must complete");
        assert_eq!(answers, 1, "no replayed or missing answer");
        assert!(client.running());
        let mut document: serde_json::Value = serde_json::from_slice(
            &std::fs::read(state).expect("transcript relocation fixture operation"),
        )
        .expect("transcript relocation fixture operation");
        document["linkScanPath"] = json!(target);
        cccc_core::fs::write_json(state, &document)
            .expect("transcript relocation fixture operation");
    }
}
