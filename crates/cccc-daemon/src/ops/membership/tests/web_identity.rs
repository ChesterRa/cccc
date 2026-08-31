use super::*;
use cccc_core::{HomeLayout, fs};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn web_ready_server(response_runtime_id: &str, proof_key: &str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind Web fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let response_runtime_id = response_runtime_id.to_owned();
    let proof_key = proof_key.to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept Web readiness");
        let mut request = [0_u8; 2048];
        let count = stream.read(&mut request).expect("read readiness request");
        let request = String::from_utf8_lossy(&request[..count]);
        assert!(!request.contains("runtime_id="));
        let target = request.split_whitespace().nth(1).expect("request target");
        let challenge = target
            .strip_prefix("/api/v1/ready?challenge=")
            .expect("challenge query");
        let proof = web_runtime_proof::sign(&proof_key, challenge).expect("proof");
        let body = json!({
            "ok":true,
            "result":{"web":"ready","runtime_id":response_runtime_id,"proof":proof}
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write readiness response");
    });
    port
}

fn reflecting_web_ready_server(response_runtime_id: &str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind Web fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let response_runtime_id = response_runtime_id.to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept Web readiness");
        let mut request = [0_u8; 2048];
        let count = stream.read(&mut request).expect("read readiness request");
        let request = String::from_utf8_lossy(&request[..count]);
        let target = request.split_whitespace().nth(1).expect("request target");
        let challenge = target
            .strip_prefix("/api/v1/ready?challenge=")
            .expect("challenge query");
        let body = json!({
            "ok":true,
            "result":{"web":"ready","runtime_id":response_runtime_id,"proof":challenge}
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write readiness response");
    });
    port
}

#[test]
fn reach_uses_the_identity_verified_live_web_port() {
    let port = web_ready_server("web_fixture", "proof-key");
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    fs::write_json(
        &home.daemon_dir().join("web_runtime.json"),
        &json!({"pid":std::process::id(),"runtime_id":"web_fixture","runtime_proof_key":"proof-key","host":"127.0.0.1","port":port}),
    )
    .expect("runtime state");
    assert_eq!(live_web_port(&home).expect("live port"), port);
}

#[test]
fn reach_rejects_a_recorded_web_port_that_is_not_listening() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind closed fixture");
    let port = listener.local_addr().expect("fixture address").port();
    drop(listener);
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    fs::write_json(
        &home.daemon_dir().join("web_runtime.json"),
        &json!({"pid":std::process::id(),"runtime_id":"web_fixture","runtime_proof_key":"proof-key","host":"127.0.0.1","port":port}),
    )
    .expect("runtime state");
    let error = live_web_port(&home).expect_err("closed binding must fail");
    assert_eq!(error.code, "membership_gate");
    assert!(error.message.contains("did not prove its runtime identity"));
}

#[test]
fn reach_rejects_a_listener_with_the_wrong_runtime_identity() {
    let port = web_ready_server("web_other", "proof-key");
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    fs::write_json(
        &home.daemon_dir().join("web_runtime.json"),
        &json!({"pid":std::process::id(),"runtime_id":"web_expected","runtime_proof_key":"proof-key","host":"127.0.0.1","port":port}),
    )
    .expect("runtime state");

    let error = live_web_port(&home).expect_err("foreign listener must fail");

    assert_eq!(error.code, "membership_gate");
    assert!(error.message.contains("did not prove its runtime identity"));
}

#[test]
fn reach_rejects_a_listener_that_reflects_the_public_challenge() {
    let port = reflecting_web_ready_server("web_expected");
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    fs::write_json(
        &home.daemon_dir().join("web_runtime.json"),
        &json!({"pid":std::process::id(),"runtime_id":"web_expected","runtime_proof_key":"proof-key","host":"127.0.0.1","port":port}),
    )
    .expect("runtime state");

    let error = live_web_port(&home).expect_err("reflected challenge must not prove identity");

    assert_eq!(error.code, "membership_gate");
    assert!(error.message.contains("did not prove its runtime identity"));
}

#[test]
fn reach_rejects_a_live_web_binding_that_cannot_accept_loopback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    fs::write_json(
        &home.daemon_dir().join("web_runtime.json"),
        &json!({"pid":std::process::id(),"runtime_id":"web_fixture","runtime_proof_key":"proof-key","host":"192.0.2.10","port":9123}),
    )
    .expect("runtime state");
    let error = live_web_port(&home).expect_err("non-loopback-only binding must fail");
    assert_eq!(error.code, "membership_gate");
    assert!(error.message.contains("127.0.0.1"));
}
