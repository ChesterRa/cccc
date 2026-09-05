use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn index() -> serde_json::Value {
    json!({"schema_version":1,"repository":"ChesterRa/cccc",
        "channels":{"stable":"1.2.3","rc":"1.3.0-rc10"}})
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn server(responses: Vec<String>) -> (reqwest::Url, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .expect("request timeout")
                .expect("accept");
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                bytes.push(stream.read_u8().await.expect("request byte"));
                assert!(bytes.len() < 8192);
            }
            requests.push(String::from_utf8(bytes).expect("HTTP request"));
            // Oversize/truncated-response tests may close the client early.
            let _ = stream.write_all(response.as_bytes()).await;
        }
        requests
    });
    (
        format!("http://{address}/").parse().expect("test URL"),
        task,
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

#[tokio::test]
async fn both_update_channels_work_when_the_releases_api_is_rate_limited() {
    let payload = index().to_string();
    let (base, task) = server(vec![
        response("403 Forbidden", "API rate limit exceeded"),
        response("200 OK", &payload),
        response("200 OK", &payload),
    ])
    .await;
    let client = client();
    assert_eq!(
        client
            .get(base.join("api/releases").expect("API fixture"))
            .send()
            .await
            .expect("limited API")
            .status(),
        403
    );
    let url = base.join("releases.json").expect("index URL");
    for (channel, expected) in [
        (ReleaseChannelArg::Stable, "1.2.3"),
        (ReleaseChannelArg::Rc, "1.3.0-rc10"),
    ] {
        assert_eq!(
            fetch_channel_version(&client, &url, "ChesterRa/cccc", channel)
                .await
                .expect("static discovery"),
            expected
        );
    }
    let requests = task.await.expect("server");
    assert!(requests[0].starts_with("GET /api/releases "));
    for request in &requests[1..] {
        assert!(request.starts_with("GET /releases.json "));
        assert!(!request.to_lowercase().contains("authorization:"));
        assert!(request.to_lowercase().contains("cache-control: no-cache"));
    }
    assert_eq!(
        release_index_url("ChesterRa/cccc", None)
            .expect("default")
            .as_str(),
        RELEASE_INDEX_URL
    );
    assert!(!RELEASE_INDEX_URL.contains("api.github.com"));
}

#[tokio::test]
async fn bad_index_responses_fail_without_a_fallback_request() {
    let oversized = "x".repeat(RELEASE_INDEX_MAX_BYTES + 1);
    for reply in [
        response("404 Not Found", "not published"),
        response("403 Forbidden", "forbidden"),
        response("200 OK", "<html>not JSON</html>"),
        "HTTP/1.1 302 Found\r\nLocation: https://api.github.com/\r\nContent-Length: 0\r\n\r\n"
            .into(),
        "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n{\"incomplete\":".into(),
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            oversized.len(),
            oversized
        ),
    ] {
        let (url, task) = server(vec![reply]).await;
        assert!(
            fetch_channel_version(&client(), &url, "ChesterRa/cccc", ReleaseChannelArg::Stable)
                .await
                .is_err()
        );
        assert_eq!(task.await.expect("server").len(), 1);
    }
}

#[test]
fn wrong_repository_schema_channel_and_version_are_rejected() {
    for patch in [
        json!({"schema_version":2}),
        json!({"repository":"another/project"}),
        json!({"channels":{"stable":null}}),
        json!({"channels":{"stable":"1.2.3-rc1"}}),
        json!({"channels":{"stable":"1.2.3/../other"}}),
        json!({"channels":{"stable":"01.2.3"}}),
    ] {
        let mut value = index();
        value
            .as_object_mut()
            .expect("index")
            .extend(patch.as_object().expect("patch").clone());
        assert!(
            index_channel_version(&value, "ChesterRa/cccc", ReleaseChannelArg::Stable).is_err()
        );
    }
    for rc in [serde_json::Value::Null, json!("1.3.0"), json!("1.3.0-01")] {
        let mut value = index();
        value["channels"]["rc"] = rc;
        assert!(index_channel_version(&value, "ChesterRa/cccc", ReleaseChannelArg::Rc).is_err());
        assert!(index_channel_version(&value, "ChesterRa/cccc", ReleaseChannelArg::Stable).is_ok());
    }
}

#[test]
fn forks_must_use_an_explicit_https_index_without_url_credentials() {
    assert!(release_index_url("other/fork", None).is_err());
    assert!(
        release_index_url(
            "other/fork",
            Some("https://other.github.io/fork/releases.json")
        )
        .is_ok()
    );
    for url in [
        "http://example.com/index",
        "file:///index",
        "https://user:secret@example.com/index",
        "https://example.com/index#fragment",
    ] {
        assert!(release_index_url("ChesterRa/cccc", Some(url)).is_err());
    }
}

#[test]
fn cached_metadata_cannot_downgrade_an_existing_channel() {
    for channel in [None, Some(ReleaseChannelArg::Stable)] {
        assert!(should_install("1.2.4", "1.2.3", channel).is_err());
        assert!(!should_install("1.2.4", "1.2.4", channel).expect("same"));
        assert!(should_install("1.2.4", "1.2.5", channel).expect("newer"));
    }
    assert!(should_install("1.3.0-rc10", "1.3.0-rc2", None).is_err());
    assert!(should_install("1.3.0-rc2", "1.3.0-rc10", None).expect("RC sequence"));
    assert!(
        !should_install("1.2.3+abc", "1.2.3+def", None).expect("metadata is not a new version")
    );
    assert!(!should_install("1.2.3-rc2", "1.2.3-rc.2", None).expect("equivalent RC notation"));
}

#[test]
fn only_an_explicit_cross_channel_switch_can_select_an_older_release() {
    assert!(
        should_install("1.3.0-rc2", "1.2.3", Some(ReleaseChannelArg::Stable)).expect("leave RC")
    );
    assert!(should_install("1.3.0", "1.3.0-rc2", Some(ReleaseChannelArg::Rc)).expect("enter RC"));
    assert!(should_install("1.3.0-rc2", "1.2.3-rc5", Some(ReleaseChannelArg::Rc)).is_err());
}
