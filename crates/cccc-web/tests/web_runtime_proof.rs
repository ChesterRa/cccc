use cccc_core::{HomeLayout, fs, web_runtime_proof};
use serde_json::Value;

#[tokio::test]
async fn live_ready_endpoint_proves_the_secret_runtime_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let runtime_path = home.daemon_dir().join("web_runtime.json");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server_home = home.clone();
    let server = tokio::spawn(async move {
        cccc_web::serve_until(server_home, "127.0.0.1", 0, async {
            let _ = shutdown_rx.await;
        })
        .await
    });

    let runtime = wait_for_runtime(&runtime_path).await;
    let port = runtime["port"].as_u64().expect("port");
    let runtime_id = runtime["runtime_id"].as_str().expect("runtime id");
    let proof_key = runtime["runtime_proof_key"].as_str().expect("proof key");
    let challenge = "caller-generated-challenge";
    let payload: Value = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/api/v1/ready"))
        .query(&[("challenge", challenge)])
        .send()
        .await
        .expect("ready response")
        .json()
        .await
        .expect("ready json");

    assert_eq!(payload["result"]["runtime_id"], runtime_id);
    let proof = payload["result"]["proof"].as_str().expect("proof");
    assert!(web_runtime_proof::verify(proof_key, challenge, proof));

    shutdown_tx.send(()).expect("shutdown");
    server.await.expect("server task").expect("server result");
}

async fn wait_for_runtime(path: &std::path::Path) -> Value {
    for _ in 0..100 {
        if let Ok(runtime) = fs::read_json(path) {
            return runtime;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("Web runtime state was not written");
}
