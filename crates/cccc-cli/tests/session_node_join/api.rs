//! A reachable loopback origin makes any attempt to contact the retired approval service observable.
use std::future::Future;
use tokio::net::TcpListener;

pub struct LocalOrigin {
    pub url: String,
    listener: TcpListener,
}

impl LocalOrigin {
    pub async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback origin");
        let url = format!("http://{}", listener.local_addr().expect("local address"));
        Self { url, listener }
    }

    pub async fn forbid_requests<T>(&self, operation: impl Future<Output = T>) -> T {
        tokio::select! {
            biased;
            _ = self.listener.accept() => panic!("terminal pairing contacted the legacy hub URL"),
            result = operation => result,
        }
    }
}
