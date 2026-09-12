mod auth_support;
use cccc_core::{GroupStore, HomeLayout, voice_recording_lease};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn external_capture_routes_without_local_models_and_releases_lease_on_start_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("external voice", "").expect("group");
    store.mutate(&group.group_id,|group|{
        group.extra.insert("assistants".into(),json!({"assistant":{"assistant_id":"voice_secretary","enabled":false,
            "config":{"recognition_backend":"external_provider_asr","external_asr_provider":"bailian"}}}));Ok(())
    }).expect("external configuration");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let app = auth_support::authenticated_app(home.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    for before_start in [false, true] {
        let lease = voice_recording_lease::update(
            &home,
            &group.group_id,
            "Group",
            &json!({"action":"acquire","owner_id":"browser"}),
        )
        .expect("lease");
        let url = format!(
            "ws://{address}/api/v1/groups/{}/assistants/voice_secretary/transcriptions/ws?owner_id=browser&lease_id={}",
            group.group_id,
            lease["lease_id"].as_str().expect("lease id")
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("upgrade");
        if before_start {
            socket
                .send(Message::Binary(vec![0, 0].into()))
                .await
                .expect("audio");
        } else {
            socket.send(Message::Text(json!({"type":"start","seq":1,"sample_rate":16000,"session_id":"test-session","capture_mode":"prompt","dispatch_target":"composer"}).to_string().into())).await.expect("start");
        }
        let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .expect("response timeout")
            .expect("response")
            .expect("frame");
        let response: Value = serde_json::from_str(message.to_text().expect("text")).expect("json");
        assert_eq!(
            response["error"]["code"],
            if before_start {
                "audio_before_start"
            } else {
                "external_asr_not_configured"
            }
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while voice_recording_lease::current(&home).expect("lease state") != json!({}) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("recording lease must be released on startup error");
    }
    server.abort();
    let _ = server.await;
}
