use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use cccc_core::{HomeLayout, settings};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

pub(super) async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "join.example.test");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request = request.header(header::CONTENT_TYPE, "application/json");
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("router");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).expect("API envelope"),
    )
}

pub(super) async fn advertised_urls(app: &Router, home: &HomeLayout, token: &str, original: &str) {
    for url in [
        "http://192.0.2.1:8848/",
        "https://localhost./",
        "https://127.0.0.1/",
        "https://[::ffff:127.0.0.1]/",
        "https://0.0.0.0/",
        "https://shared.example.test/ui/",
    ] {
        settings::update(home, |s| {
            s.remote_access.insert("web_public_url".into(), json!(url));
            Ok(())
        })
        .expect("isolated advertised setting");
        let (status, value) = call(
            app,
            "GET",
            "/api/v1/session_nodes",
            Some(token),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["result"]["join_supported"], false);
        assert_eq!(value["result"]["join_url"], Value::Null);
    }
    settings::update(home, |s| {
        s.remote_access
            .insert("web_public_url".into(), json!(original));
        Ok(())
    })
    .expect("restore fixture route");
}
