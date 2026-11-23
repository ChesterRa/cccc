//! Real I/O phases for paused-clock lifecycle tests. A blocking observer waits
//! for the actual async task, inhibiting automatic clock advance during I/O.
//! Dropping the observation cancels accept/read, including on assertion failure.
use crate::PRODUCT_VERSION;
use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use cccc_core::HomeLayout;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::{AbortHandle, JoinError, JoinHandle};

pub(super) const ISOLATED_DAEMON_CASE: &str = "CCCC_TEST_ISOLATED_DAEMON_CASE";

pub(super) fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    (temp, home)
}

fn tcp_address(address: std::net::SocketAddr) -> DaemonAddress {
    DaemonAddress {
        v: 1,
        transport: Transport::Tcp,
        path: String::new(),
        host: "127.0.0.1".into(),
        port: address.port(),
        pid: std::process::id(),
        version: PRODUCT_VERSION.into(),
        source_id: String::new(),
        ts: String::new(),
    }
}

pub(super) type Peer = BufReader<TcpStream>;

pub(super) struct ObservedTask<T> {
    abort: AbortHandle,
    completed: JoinHandle<Result<T, JoinError>>,
}

impl<T> ObservedTask<T> {
    pub(super) fn abort(&self) {
        self.abort.abort();
    }
}

impl<T> Future for ObservedTask<T> {
    type Output = Result<T, JoinError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().completed)
            .poll(cx)
            .map(|result| result.expect("I/O observation task"))
    }
}

impl<T> Drop for ObservedTask<T> {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

pub(super) fn task<T: Send + 'static>(operation: JoinHandle<T>) -> ObservedTask<T> {
    let abort = operation.abort_handle();
    let runtime = tokio::runtime::Handle::current();
    let completed = tokio::task::spawn_blocking(move || runtime.block_on(operation));
    ObservedTask { abort, completed }
}

pub(super) async fn listener(home: &HomeLayout) -> Arc<TcpListener> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener");
    std::fs::write(
        cccc_client::address_path(home),
        serde_json::to_vec(&tcp_address(listener.local_addr().expect("address")))
            .expect("address JSON"),
    )
    .expect("publish daemon");
    Arc::new(listener)
}

pub(super) async fn ping_read(listener: &Arc<TcpListener>) -> Peer {
    let listener = Arc::clone(listener);
    task(tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("ping connection");
        let mut peer = BufReader::new(stream);
        let mut line = String::new();
        peer.read_line(&mut line).await.expect("ping request");
        let request: DaemonRequest = serde_json::from_str(&line).expect("request JSON");
        assert_eq!(request.op, "ping");
        peer
    }))
    .await
    .expect("read request task")
}

pub(super) fn reply(
    peer: Peer,
    response: DaemonResponse,
) -> (oneshot::Receiver<()>, ObservedTask<()>) {
    send_reply(peer, response, false)
}

pub(super) fn late_reply(
    peer: Peer,
    response: DaemonResponse,
) -> (oneshot::Receiver<()>, ObservedTask<()>) {
    send_reply(peer, response, true)
}

fn send_reply(
    mut peer: Peer,
    response: DaemonResponse,
    may_be_closed: bool,
) -> (oneshot::Receiver<()>, ObservedTask<()>) {
    let (written, observed) = oneshot::channel();
    let closed = task(tokio::spawn(async move {
        let mut bytes = serde_json::to_vec(&response).expect("reply JSON");
        bytes.push(b'\n');
        if let Err(error) = peer.write_all(&bytes).await {
            assert!(
                may_be_closed && is_closed(error.kind()),
                "write reply: {error}"
            );
            let _ = written.send(());
            return;
        }
        let _ = written.send(());
        assert_closed(&mut peer).await;
    }));
    (observed, closed)
}

async fn assert_closed(peer: &mut Peer) {
    let mut tail = String::new();
    match peer.read_line(&mut tail).await {
        Ok(0) => {}
        // Closing with an unread late reply may reset TCP instead of sending FIN.
        Err(error) if is_closed(error.kind()) => {}
        result => panic!("expected a terminal connection close, got {result:?}"),
    }
}

fn is_closed(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

pub(super) async fn advance_past(duration: std::time::Duration) {
    // Observe the timer driver too: advance() does not promise timers have run.
    let boundary = tokio::time::sleep_until(tokio::time::Instant::now() + duration);
    tokio::pin!(boundary);
    assert!(poll_once(boundary.as_mut()).await.is_pending());
    // Tokio documents millisecond timer granularity. Cross one logical tick,
    // rather than extending a wall-clock wait or assuming scheduling progress.
    tokio::time::advance(duration + std::time::Duration::from_millis(1)).await;
    boundary.await;
}

pub(super) async fn closed(mut peer: Peer) {
    task(tokio::spawn(async move { assert_closed(&mut peer).await }))
        .await
        .expect("closed connection task");
}

pub(super) async fn reject(mut peer: Peer) {
    task(tokio::spawn(async move {
        peer.shutdown().await.expect("end reply side");
        assert_closed(&mut peer).await;
    }))
    .await
    .expect("rejected connection task");
}

pub(super) async fn poll_once<F: Future>(mut operation: Pin<&mut F>) -> Poll<F::Output> {
    poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await
}

pub(super) async fn pending_until<F: Future, E: Future>(
    mut operation: Pin<&mut F>,
    event: E,
) -> E::Output {
    tokio::select! {
        biased;
        _ = operation.as_mut() => panic!("operation completed before the observed I/O phase"),
        value = event => value,
    }
}

pub(super) async fn daemon_published<F: Future>(mut daemon: Pin<&mut F>, home: &HomeLayout) {
    let path = cccc_client::address_path(home);
    let previous = std::fs::read(&path).ok();
    poll_fn(|cx| {
        assert!(
            daemon.as_mut().poll(cx).is_pending(),
            "daemon ended during startup"
        );
        let current = std::fs::read(&path).ok();
        if current.is_some() && current != previous {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}
