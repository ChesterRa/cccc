use super::*;
use std::time::Duration;
use tokio::net::TcpStream;

#[tokio::test]
async fn idle_preconnection_does_not_block_other_fixture_requests() {
    let (url, server) = local_page("fixture response").await;
    let address = url.strip_prefix("http://").expect("HTTP fixture");
    let mut idle = TcpStream::connect(address).await.expect("preconnection");
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut request = TcpStream::connect(address).await.expect("request");
        request
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("send request");
        let mut response = String::new();
        request
            .read_to_string(&mut response)
            .await
            .expect("response");
        response
    })
    .await;
    server.abort();
    let _ = server.await;
    let mut byte = [0];
    let closed = tokio::time::timeout(Duration::from_secs(2), idle.read(&mut byte))
        .await
        .expect("stopping the fixture must close idle connections")
        .expect("idle connection closed");
    assert_eq!(closed, 0);
    let response = result.expect("an idle preconnection must not block another request");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("fixture response"));
}
