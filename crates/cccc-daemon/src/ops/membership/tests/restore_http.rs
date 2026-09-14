#[cfg(unix)]
use cccc_core::{HomeLayout, fs, web_runtime_proof};
#[cfg(unix)]
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

pub(super) struct HttpFixture {
    pub(super) port: u16,
    stop: Arc<AtomicBool>,
    pub(super) requests: Arc<Mutex<Vec<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HttpFixture {
    pub(super) fn new(handler: impl Fn(&str) -> (u16, String) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let port = listener.local_addr().expect("address").port();
        listener.set_nonblocking(true).expect("nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                // On BSD/macOS accepted sockets inherit the listener's nonblocking mode.
                // A read timeout alone does not restore blocking request reads.
                stream.set_nonblocking(false).expect("blocking connection");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("timeout");
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = stream.read(&mut buffer).expect("request");
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                        let length = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .map_or(0, |length| length.trim().parse::<usize>().expect("length"));
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).expect("utf8");
                captured.lock().expect("capture").push(request.clone());
                let (status, body) = handler(&request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            port,
            stop,
            requests,
            worker: Some(worker),
        }
    }

    pub(super) fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    #[cfg(unix)]
    pub(super) fn web(home: &HomeLayout) -> Self {
        let fixture = Self::new(|request| {
            let target = request.split_whitespace().nth(1).expect("target");
            let challenge = target
                .strip_prefix("/api/v1/ready?challenge=")
                .expect("challenge");
            let proof = web_runtime_proof::sign("isolated-proof", challenge).expect("proof");
            (200, json!({"ok":true,"result":{"web":"ready","runtime_id":"fixture-web","proof":proof}}).to_string())
        });
        fs::write_json(&home.daemon_dir().join("web_runtime.json"), &json!({
            "pid":std::process::id(), "runtime_id":"fixture-web", "runtime_proof_key":"isolated-proof",
            "host":"127.0.0.1", "port":fixture.port
        })).expect("web identity");
        fixture
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .expect("worker")
            .join()
            .expect("fixture finished");
    }
}

#[test]
fn receives_a_delayed_fragmented_request_before_responding() {
    use std::net::TcpStream;
    let fixture = HttpFixture::new(|request| {
        assert!(request.ends_with("payload"));
        (200, "accepted".into())
    });
    let mut client = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("response timeout");
    // Keep a live accepted connection idle, then deliver the HTTP body separately.
    // Nonblocking accepted sockets used to make the fixture worker panic here.
    thread::sleep(Duration::from_millis(100));
    client
        .write_all(b"POST /restore HTTP/1.1\r\nHost: fixture\r\nContent-Length: 7\r\n\r\npay")
        .expect("partial request");
    thread::sleep(Duration::from_millis(100));
    client.write_all(b"load").expect("remaining body");
    let mut response = String::new();
    client.read_to_string(&mut response).expect("response");
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.ends_with("accepted"));
    let requests = fixture.requests.lock().expect("captured request");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].ends_with("payload"));
}
