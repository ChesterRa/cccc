use super::protocol::{
    ActorSnapshot, CONTRACT, ExecutionFact, Failure, MAX_FRAME_BYTES, Mutation, OperationId,
    OperationSource, OperationState, OwnerSnapshot, Request, Response,
};
use super::transport::{Address, Paths, SocketPath};
use cccc_core::HomeLayout;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) struct Client {
    pub address: Address,
    identified_source: bool,
    same_source: bool,
    control: Mutex<Option<Control>>,
}

struct Control {
    connection: Connection,
    epoch: u64,
    next_sequence: u64,
}

struct Connection(BufReader<UnixStream>);

#[cfg(all(test, target_os = "linux"))]
#[test]
fn full_owner_accept_queue_rejects_connect_before_waiting_for_io() {
    let temp = tempfile::tempdir().expect("private socket directory");
    let address = Address {
        owner_id: "fixture".into(),
        instance: "fixture".into(),
        contract: CONTRACT,
        pid: std::process::id(),
        socket: temp.path().join("control.sock"),
    };
    let listener = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)
        .expect("fixture socket");
    listener
        .bind(&socket2::SockAddr::unix(&address.socket).expect("socket address"))
        .expect("bind");
    listener.listen(0).expect("one queued connection");
    drop(UnixStream::connect(&address.socket).expect("fill the accept queue"));

    // Release the slot if a regression would otherwise hang this test.
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let drain = std::thread::spawn(move || {
        let _ = wait.recv_timeout(Duration::from_secs(2));
        listener.accept().expect("drain the queued connection");
        listener
    });
    let rejected = Connection::connect(&address);
    let _ = done.send(());
    let listener = drain.join().expect("release accept queue");
    assert_eq!(
        rejected.err().expect("queue backpressure").kind(),
        io::ErrorKind::WouldBlock
    );
    Connection::connect(&address).expect("connect after the owner accepts again");
    listener.accept().expect("accept recovered connection");
}

impl Client {
    pub(super) fn connect(address: Address) -> io::Result<Self> {
        Self::connect_with_build(address).map(|(client, _)| client)
    }

    fn connect_with_build(address: Address) -> io::Result<(Self, serde_json::Value)> {
        let mut connection = Connection::connect(&address)?;
        let build = connection.verify(&address)?;
        let client = Self {
            address,
            identified_source: build
                .get("source_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|source| {
                    source.len() == 64 && source.bytes().all(|byte| byte.is_ascii_hexdigit())
                }),
            same_source: same_execution_source(cccc_core::build_info::source_id(), &build),
            control: Mutex::new(None),
        };
        Ok((client, build))
    }

    pub(super) fn supports_target(
        &self,
        owner: &crate::ops::actor_runtime::RegisteredOwner,
    ) -> bool {
        use crate::ops::actor_runtime::OwnerIdentity;
        // A kept PTY is bound by the old contract and this controller cannot
        // start its replacement in a different execution source. The other
        // backends need the same-source execution fences for targeted writes.
        self.same_source
            || (self.identified_source && matches!(owner.identity, OwnerIdentity::Pty { .. }))
    }

    /// A read never acquires or replaces the mutating stream.
    pub(super) fn current_start_factory(&self) -> io::Result<bool> {
        match self.read(Request::Hello { contract: CONTRACT })? {
            Response::Hello {
                accepting_starts,
                build,
                ..
            } => Ok(accepting_starts
                && same_execution_source(cccc_core::build_info::source_id(), &build)),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn snapshots(&self) -> io::Result<Vec<ActorSnapshot>> {
        Ok(self.owner_snapshot()?.actors)
    }

    pub(super) fn owner_snapshot(&self) -> io::Result<OwnerSnapshot> {
        match self.read(Request::Snapshot)? {
            Response::Snapshot { mut snapshot } => {
                self.check_browser_holders(&snapshot)?;
                for actor in &mut snapshot.actors {
                    if let Some(error) = &mut actor.observation_error {
                        error.message = format!(
                            "session owner {} (PID {}) could not confirm member {}/{} ({}){}",
                            self.address.owner_id,
                            self.address.pid,
                            actor.actor.group_id,
                            actor.actor.actor_id,
                            error.code,
                            if error.code == "detached_work_unconfirmed" {
                                format!(
                                    "; first leave the daemon with `cccc daemon stop --keep-agents`, then review `cccc daemon process-recovery --owner session --owner-id {} list`; accepting risk does not confirm job removal",
                                    self.address.owner_id
                                )
                            } else {
                                "; retry observation or use `cccc daemon stop`; stop is not confirmed".into()
                            }
                        );
                    }
                    if let Some(terminal) = &mut actor.terminal {
                        terminal.launch = actor.terminal_launch;
                    }
                }
                Ok(snapshot)
            }
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    fn check_browser_holders(&self, snapshot: &OwnerSnapshot) -> io::Result<()> {
        if snapshot.browsers.iter().any(|browser| {
            browser.resource.owner_id != self.address.owner_id
                || browser.resource.instance != self.address.instance
                || browser.resource.resource_id.is_empty()
        }) {
            return Err(io::Error::other(
                "browser observation holder identity changed",
            ));
        }
        Ok(())
    }

    pub(super) fn browsers(&self) -> io::Result<Vec<super::browser::BrowserResource>> {
        match self.read(Request::BrowserResources)? {
            Response::BrowserResources { resources } => Ok(resources),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    /// One read-only, identity-checked exchange. The whole deadline includes
    /// connect, so a full listener queue cannot block an update indefinitely.
    pub(super) async fn observe(&self, deadline: Instant) -> io::Result<(u32, OwnerSnapshot)> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owner observation deadline elapsed",
                )
            })?;
        let exchange = async {
            let (mut stream, registration) = verified_stream(&self.address).await?;
            match observe_exchange(&mut stream, &Request::Snapshot).await? {
                Response::Snapshot { snapshot } => {
                    self.check_browser_holders(&snapshot)?;
                    Ok((registration, snapshot))
                }
                Response::Failed { error } => Err(failure(error)),
                _ => Err(invalid_response()),
            }
        };
        tokio::time::timeout(remaining, exchange)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "session owner observation timed out",
                )
            })?
            .map_err(|error| with_owner(&self.address, error))
    }

    pub(super) fn projected_snapshots(&self) -> io::Result<Vec<ActorSnapshot>> {
        if let Some(snapshot) = super::projection::get(&self.address.instance) {
            return snapshot;
        }
        let snapshot = self.snapshots();
        super::projection::remember(&self.address.instance, &snapshot);
        snapshot
    }

    pub(super) fn query(&self, operation: &OperationId) -> io::Result<OperationState> {
        match self.read(Request::Query {
            operation: operation.clone(),
        })? {
            Response::Operation { state } => Ok(state),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn find(&self, source: &OperationSource) -> io::Result<Vec<OperationId>> {
        match self.read(Request::Find {
            source: source.clone(),
        })? {
            Response::Found { operations } => Ok(operations),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn find_event(
        &self,
        group: &str,
        actor: &str,
        created: &str,
        event: &str,
    ) -> io::Result<Vec<ExecutionFact>> {
        match self.read(Request::FindEvent {
            group_id: group.into(),
            actor_id: actor.into(),
            actor_created_at: created.into(),
            event_id: event.into(),
        })? {
            Response::FoundEvent { operations } => Ok(operations),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn terminal_read(
        &self,
        actor: super::protocol::ActorRef,
        read: super::protocol::TerminalRead,
    ) -> io::Result<serde_json::Value> {
        match self.read(Request::TerminalRead { actor, read })? {
            Response::Terminal { value } => Ok(value),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    fn read(&self, request: Request) -> io::Result<Response> {
        let mut connection = Connection::connect(&self.address)?;
        connection.verify(&self.address)?;
        connection
            .exchange(&request)
            .map_err(|error| with_owner(&self.address, error))
    }

    /// This lock covers allocation and the actual stream write. Concurrent
    /// callers cannot assign numbers in a different order from their writes.
    /// On exchange failure the operation is uncertain; it is never resent.
    pub(super) fn mutate(&self, mutation: Mutation) -> io::Result<OperationId> {
        self.mutate_tracked(mutation, |_| Ok(()))
    }

    pub(super) fn mutate_tracked(
        &self,
        mutation: Mutation,
        remember: impl FnOnce(&OperationId) -> io::Result<()>,
    ) -> io::Result<OperationId> {
        match self.change_tracked(mutation, remember)? {
            Response::Admitted { operation } => Ok(operation),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn acknowledge(&self, operation: &OperationId) -> io::Result<()> {
        match self.change(Mutation::Acknowledge {
            operation: operation.clone(),
        })? {
            Response::Acknowledged => Ok(()),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn retire(&self) -> io::Result<()> {
        match self.change(Mutation::Retire)? {
            Response::Retired => Ok(()),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    fn change(&self, mutation: Mutation) -> io::Result<Response> {
        self.change_tracked(mutation, |_| Ok(()))
    }

    pub(super) fn recover(&self, action: super::RecoveryAction) -> io::Result<serde_json::Value> {
        let response = if matches!(action, super::RecoveryAction::List) {
            self.read(Request::RecoveryList)?
        } else {
            self.change(Mutation::Recovery { action })?
        };
        match response {
            Response::Recovery { value } => Ok(value),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn take_control(&self) -> io::Result<()> {
        let mut current = self
            .control
            .lock()
            .map_err(|_| io::Error::other("owner client lock poisoned"))?;
        self.acquire(&mut current)
    }

    pub(super) fn control_epoch(&self) -> io::Result<u64> {
        let mut current = self
            .control
            .lock()
            .map_err(|_| io::Error::other("owner client lock poisoned"))?;
        self.acquire(&mut current)?;
        Ok(current.as_ref().expect("control acquired").epoch)
    }

    fn acquire(&self, current: &mut Option<Control>) -> io::Result<()> {
        if current.is_some() {
            return Ok(());
        }
        let mut connection = Connection::connect(&self.address)?;
        connection.verify(&self.address)?;
        let epoch = match connection
            .exchange(&Request::Acquire {
                contract: CONTRACT,
                instance: self.address.instance.clone(),
            })
            .map_err(|error| with_owner(&self.address, error))?
        {
            Response::Acquired { epoch } => epoch,
            Response::Failed { error } => return Err(failure(error)),
            _ => return Err(invalid_response()),
        };
        *current = Some(Control {
            connection,
            epoch,
            next_sequence: 0,
        });
        Ok(())
    }

    fn change_tracked(
        &self,
        mutation: Mutation,
        remember: impl FnOnce(&OperationId) -> io::Result<()>,
    ) -> io::Result<Response> {
        super::projection::invalidate();
        let mut current = self
            .control
            .lock()
            .map_err(|_| io::Error::other("owner client lock poisoned"))?;
        self.acquire(&mut current)?;
        let control = current.as_mut().expect("control installed");
        control.next_sequence = control
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("owner sequence exhausted"))?;
        remember(&OperationId {
            instance: self.address.instance.clone(),
            epoch: control.epoch,
            sequence: control.next_sequence,
        })?;
        let response = control
            .connection
            .exchange(&Request::Mutate {
                epoch: control.epoch,
                sequence: control.next_sequence,
                mutation: Box::new(mutation),
            })
            .map_err(|error| with_owner(&self.address, error));
        if response.is_err()
            || matches!(&response, Ok(Response::Failed { error }) if error.code == "stale_epoch")
        {
            // A competing recovery/control stream can fence this connection.
            // Fail this call without replay, then acquire afresh on the next.
            current.take();
        }
        response
    }

    pub(super) fn wait(
        &self,
        operation: &OperationId,
        timeout: Duration,
    ) -> io::Result<serde_json::Value> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.query(operation)? {
                OperationState::Finished { result } => return result.map_err(failure),
                OperationState::Pending if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                OperationState::Pending => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "owner operation is still running; do not replay input",
                    ));
                }
                OperationState::NotAdmitted => {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "owner operation was not admitted",
                    ));
                }
                OperationState::Unknown => {
                    return Err(io::Error::other(
                        "owner operation outcome is unknown; do not replay input",
                    ));
                }
            }
        }
    }

    pub(super) fn release(&self) -> io::Result<()> {
        self.control
            .lock()
            .map_err(|_| io::Error::other("owner client lock poisoned"))?
            .take();
        Ok(())
    }
}

/// Source identity chooses the code for a new launch, not protocol compatibility.
fn same_execution_source(current: Option<&str>, owner: &serde_json::Value) -> bool {
    current.is_some_and(|source| {
        source.len() == 64
            && source.bytes().all(|byte| byte.is_ascii_hexdigit())
            && owner.get("source_id").and_then(serde_json::Value::as_str) == Some(source)
    })
}

#[cfg(test)]
mod current_start_factory_tests {
    use super::same_execution_source;
    use serde_json::json;

    #[test]
    fn a_factory_requires_the_current_execution_source_not_the_version_label() {
        let source = "a".repeat(64);
        assert!(same_execution_source(
            Some(&source),
            &json!({"source_id":source})
        ));
        assert!(!same_execution_source(
            Some(&source),
            &json!({"source_id":"b".repeat(64),"version":"0.4.58"})
        ));
        assert!(!same_execution_source(
            Some(&source),
            &json!({"version":"0.4.58"})
        ));
        assert!(!same_execution_source(None, &json!({})));
        assert!(!same_execution_source(Some(""), &json!({"source_id":""})));
        assert!(!same_execution_source(
            Some(&source),
            &json!({"source_id":4})
        ));
    }

    #[cfg(all(
        debug_assertions,
        any(target_os = "linux", all(target_os = "macos", target_arch = "aarch64"))
    ))]
    #[test]
    fn a_new_owner_with_a_different_source_is_reaped_before_admission() {
        let directory = tempfile::tempdir().expect("private home");
        let home = cccc_core::HomeLayout::from_path(directory.path().join("home")).expect("home");
        home.initialize().expect("initialize home");
        let _home_lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
            .expect("check home lock")
            .expect("hold home lock");
        let id = uuid::Uuid::new_v4().simple().to_string();
        let error = super::start_with_source(&home, &id, Some(&"a".repeat(64)))
            .err()
            .expect("fixture owner does not have the requested build");
        assert!(error.to_string().contains("restart CCCC"));
        let replacement = super::start(&home, &id).expect("start under the released owner lock");
        assert!(replacement.snapshots().expect("read members").is_empty());
        replacement.retire().expect("retire the empty fixture");
    }
}

/// Whether the holder at this address answers Hello within the deadline,
/// which also covers connect. It registers nothing and changes nothing.
pub(super) async fn answers(address: &Address, deadline: Instant) -> io::Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match tokio::time::timeout(remaining, verified_stream(address)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(with_owner(address, error)),
        Err(_) => Err(with_owner(
            address,
            io::Error::new(io::ErrorKind::TimedOut, "no reply to Hello"),
        )),
    }
}

/// Connect for read-only requests after checking the holder's identity and
/// contract. The caller bounds the time, including connect.
async fn verified_stream(
    address: &Address,
) -> io::Result<(tokio::io::BufReader<tokio::net::UnixStream>, u32)> {
    let socket = SocketPath::new(&address.socket)?;
    let stream = tokio::net::UnixStream::connect(&socket.path).await?;
    drop(socket);
    let mut stream = tokio::io::BufReader::new(stream);
    match observe_exchange(&mut stream, &Request::Hello { contract: CONTRACT }).await? {
        Response::Hello {
            contract,
            instance,
            pid,
            mcp_registration,
            ..
        } if contract == CONTRACT && instance == address.instance && pid == address.pid => {
            Ok((stream, mcp_registration))
        }
        Response::Failed { error } => Err(failure(error)),
        _ => Err(io::Error::other(
            "session owner identity or contract changed",
        )),
    }
}

async fn observe_exchange(
    stream: &mut tokio::io::BufReader<tokio::net::UnixStream>,
    request: &Request,
) -> io::Result<Response> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let mut frame = serde_json::to_vec(request).map_err(io::Error::other)?;
    if frame.len() >= MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "owner request exceeds frame limit",
        ));
    }
    frame.push(b'\n');
    stream.get_mut().write_all(&frame).await?;
    let mut response = Vec::new();
    (&mut *stream)
        .take(MAX_FRAME_BYTES as u64 + 1)
        .read_until(b'\n', &mut response)
        .await?;
    if response.len() > MAX_FRAME_BYTES || response.last() != Some(&b'\n') {
        return Err(invalid_response());
    }
    serde_json::from_slice(&response).map_err(io::Error::other)
}

#[cfg(test)]
mod finite_observation_tests {
    use super::*;
    use tokio::io::AsyncBufReadExt;

    #[tokio::test]
    async fn an_observation_that_connects_but_gets_no_hello_reply_times_out() {
        let temp = tempfile::tempdir().expect("private socket");
        let socket = temp.path().join("owner.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("listen");
        let (seen, observed) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut stream = tokio::io::BufReader::new(stream);
            let mut request = String::new();
            stream.read_line(&mut request).await.expect("hello frame");
            assert!(matches!(
                serde_json::from_str::<Request>(&request).expect("request"),
                Request::Hello { .. }
            ));
            seen.send(()).expect("observed hello");
            request.clear();
            assert_eq!(
                stream
                    .read_line(&mut request)
                    .await
                    .expect("peer closes on deadline"),
                0
            );
        });
        let client = Client {
            address: Address {
                owner_id: uuid::Uuid::new_v4().simple().to_string(),
                instance: uuid::Uuid::new_v4().simple().to_string(),
                contract: CONTRACT,
                pid: std::process::id(),
                socket,
            },
            identified_source: false,
            same_source: false,
            control: Mutex::new(None),
        };
        let result = client.observe(Instant::now() + Duration::from_secs(1));
        tokio::pin!(result);
        tokio::select! {
            observed = observed => observed.expect("fixture received the actual request"),
            result = &mut result => panic!("request ended before the fixture saw hello: {:?}", result.err()),
        }
        assert_eq!(
            result.await.err().expect("no reply").kind(),
            io::ErrorKind::TimedOut
        );
        server.await.expect("fixture finishes");
    }
}

impl Connection {
    fn connect(address: &Address) -> io::Result<Self> {
        let socket = (|| {
            let path = SocketPath::new(&address.socket)?;
            let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
            // A suspended owner can fill its accept queue even after those
            // clients timed out and closed. Bound connect before the first I/O.
            socket.set_nonblocking(true)?;
            socket.connect(&socket2::SockAddr::unix(&path.path)?)?;
            socket.set_nonblocking(false)?;
            Ok::<UnixStream, io::Error>(socket.into())
        })()
        .map_err(|error| with_owner(address, error))?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self(BufReader::new(socket)))
    }

    fn verify(&mut self, address: &Address) -> io::Result<serde_json::Value> {
        match self
            .exchange(&Request::Hello { contract: CONTRACT })
            .map_err(|error| with_owner(address, error))?
        {
            Response::Hello {
                contract,
                instance,
                pid,
                build,
                ..
            } if contract == CONTRACT && instance == address.instance && pid == address.pid => {
                Ok(build)
            }
            Response::Failed { error } => Err(failure(error)),
            _ => Err(io::Error::other(
                "session owner identity or contract changed",
            )),
        }
    }

    fn exchange(&mut self, request: &Request) -> io::Result<Response> {
        let mut bytes = serde_json::to_vec(request).map_err(io::Error::other)?;
        if bytes.len() >= MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owner request exceeds frame limit",
            ));
        }
        bytes.push(b'\n');
        self.0.get_mut().write_all(&bytes)?;
        self.0.get_mut().flush()?;
        let mut response = Vec::new();
        let size = (&mut self.0)
            .take(MAX_FRAME_BYTES as u64 + 1)
            .read_until(b'\n', &mut response)?;
        if size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "owner closed the connection",
            ));
        }
        if size > MAX_FRAME_BYTES || response.last() != Some(&b'\n') {
            return Err(invalid_response());
        }
        serde_json::from_slice(&response).map_err(io::Error::other)
    }
}

/// Called only on an authorized start path while the controller owns its home
/// lock. Status and startup discovery never construct a replacement owner.
pub(super) fn start(home: &HomeLayout, owner_id: &str) -> io::Result<Client> {
    start_with_source(home, owner_id, cccc_core::build_info::source_id())
}

fn start_with_source(
    home: &HomeLayout,
    owner_id: &str,
    expected_source: Option<&str>,
) -> io::Result<Client> {
    let paths = Paths::new(home, owner_id)?;
    paths.initialize()?;
    // Holding the owner lock proves an old process no longer owns it; release
    // before spawning, because the child takes that same lock before binding.
    let lock = cccc_core::fs::try_exclusive_lock(&paths.lock)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "session owner is running but unavailable",
        )
    })?;
    drop(lock);
    let executable = std::env::current_exe()?;
    let name = executable
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let name = name.strip_suffix(" (deleted)").unwrap_or(name);
    let public_entry = name == "cccc";
    #[cfg(debug_assertions)]
    let test_entry = !public_entry
        && executable
            .parent()
            .and_then(|path| path.file_name())
            .is_some_and(|name| name == "deps")
        && name.rsplit_once('-').is_some_and(|(_, suffix)| {
            suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    #[cfg(not(debug_assertions))]
    let test_entry = false;
    if !public_entry && !test_entry {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "CCCC could not start its session-owner entry from {}; run this program from a file named cccc, or provide a session-owner entry when embedding CCCC",
                executable.display()
            ),
        ));
    }
    #[cfg(target_os = "linux")]
    let mut command = {
        use std::os::unix::process::CommandExt;
        let mut command = cccc_runtime::background_command("/proc/self/exe");
        command.arg0("cccc");
        command
    };
    #[cfg(not(target_os = "linux"))]
    let mut command = cccc_runtime::background_command(executable);
    if public_entry {
        command.args(["daemon", "session-owner", "--owner-id", owner_id]);
    }
    #[cfg(debug_assertions)]
    if test_entry {
        command.args(["__cccc_session_owner_child", "--ignored", "--nocapture"]);
    }
    command
        // The owner has no old controller environment to leak into a later
        // Start. Each member receives its freshly prepared launch snapshot.
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CCCC_HOME", home.root())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home_path) = std::env::var_os("HOME") {
        command.env("HOME", home_path);
    }
    #[cfg(debug_assertions)]
    if test_entry {
        command.env("CCCC_TEST_SESSION_OWNER_CHILD", owner_id);
    }
    let mut child = command
        // Do not wrap this process in the daemon's OwnedProcessTree.
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "session owner exited before readiness: {status}"
            )));
        }
        if let Ok(address) = load(home, owner_id)
            && address.pid == child.id()
            && let Ok((client, build)) = Client::connect_with_build(address)
        {
            if let Some(source) = expected_source
                && !same_execution_source(Some(source), &build)
            {
                // The launch path can be replaced while this daemon runs. This
                // direct child has not been handed any member or browser.
                let _ = child.kill();
                child.wait()?;
                return Err(io::Error::other(
                    "the new session owner is from a different build than the running daemon; restart CCCC before starting new agents or browsers",
                ));
            }
            // Reap on eventual exit without retaining kill-on-drop ownership.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(client);
        }
        if Instant::now() >= deadline {
            // Only our unready direct child is eligible for this cleanup.
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "session owner did not become ready",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn load(home: &HomeLayout, owner_id: &str) -> io::Result<Address> {
    let paths = Paths::new(home, owner_id)?;
    let address: Address = cccc_core::fs::read_json(&paths.address)?;
    if address.owner_id != paths.owner_id
        || address.contract != CONTRACT
        || address.socket != paths.socket
    {
        return Err(io::Error::other("invalid session owner discovery record"));
    }
    Ok(address)
}

fn failure(error: Failure) -> io::Error {
    io::Error::other(error)
}

fn with_owner(address: &Address, error: io::Error) -> io::Error {
    if error.get_ref().is_some_and(|value| value.is::<Failure>()) {
        return error;
    }
    let owner_exited = cccc_core::local_sessions::NativeHost::capture(address.pid)
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
    let recovery = format!(
        "unconfirmed work may require `cccc daemon process-recovery --owner session --owner-id {} list`",
        address.owner_id
    );
    let guidance = if owner_exited {
        format!(
            "the owner has exited; start CCCC again to inspect and reconcile its saved recovery state; {recovery}"
        )
    } else {
        format!(
            "inspect that owner; if it is still running, retry `cccc daemon stop` after it responds; if it has exited, start CCCC again to inspect its saved recovery state; {recovery}"
        )
    };
    io::Error::new(
        error.kind(),
        format!(
            "session owner {} (PID {}, instance {}) could not be verified ({:?}); agents may still be running; {}",
            address.owner_id,
            address.pid,
            address.instance,
            error.kind(),
            guidance,
        ),
    )
}

fn invalid_response() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid session owner response")
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn request(stream: &mut BufReader<UnixStream>) -> Request {
        let mut line = String::new();
        stream
            .read_line(&mut line)
            .expect("read test protocol frame");
        serde_json::from_str(&line).expect("decode test protocol frame")
    }

    fn respond(stream: &mut BufReader<UnixStream>, value: Response) {
        serde_json::to_writer(stream.get_mut(), &value).expect("encode test protocol frame");
        stream
            .get_mut()
            .write_all(b"\n")
            .expect("write test protocol frame");
    }

    #[test]
    fn an_unreachable_owner_keeps_unknown_and_exited_guidance_separate() {
        let mut address = Address {
            owner_id: "fixture-owner".into(),
            instance: "fixture-instance".into(),
            contract: CONTRACT,
            pid: std::process::id(),
            socket: "/isolated/unused.sock".into(),
        };
        let error = with_owner(&address, io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(error.to_string().contains("if it is still running"));
        assert!(!error.to_string().contains("the owner has exited"));

        // Hold the exited child unreaped, so its PID cannot be reused.
        let mut ended = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("owned child");
        let pid = rustix::process::Pid::from_raw(ended.id() as i32).expect("child PID");
        rustix::process::waitid(
            rustix::process::WaitId::Pid(pid),
            rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::NOWAIT,
        )
        .expect("wait without reaping");
        address.pid = ended.id();
        let error = with_owner(&address, io::Error::from(io::ErrorKind::BrokenPipe));
        ended.wait().expect("reap the fixture");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(error.to_string().contains("the owner has exited"));
        assert!(error.to_string().contains("saved recovery state"));
        assert!(!error.to_string().contains("after it responds"));

        let error = with_owner(
            &address,
            failure(Failure {
                code: "fixture_refusal".into(),
                message: "typed refusal".into(),
            }),
        );
        assert_eq!(
            error
                .get_ref()
                .expect("failure")
                .downcast_ref::<Failure>()
                .expect("preserve the typed refusal")
                .code,
            "fixture_refusal"
        );
    }

    #[test]
    fn a_fenced_call_is_not_replayed_and_the_next_call_reacquires_control() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let socket = temp.path().join("owner.sock");
        let listener = UnixListener::bind(&socket).expect("bind fake owner socket");
        let (old_client, old_server) = UnixStream::pair().expect("connect control stream pair");
        old_server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set test socket timeout");
        let address = Address {
            owner_id: uuid::Uuid::new_v4().simple().to_string(),
            instance: "live-instance".into(),
            contract: CONTRACT,
            pid: std::process::id(),
            socket,
        };
        let server_address = address.clone();
        let server = std::thread::spawn(move || {
            let mut old = BufReader::new(old_server);
            assert!(matches!(
                request(&mut old),
                Request::Mutate {
                    epoch: 1,
                    sequence: 1,
                    ..
                }
            ));
            respond(
                &mut old,
                Response::Failed {
                    error: Failure::new("stale_epoch", "another controller took over"),
                },
            );
            let mut extra = String::new();
            assert_eq!(
                old.read_line(&mut extra).expect("read test protocol frame"),
                0,
                "the rejected call was not replayed"
            );
            let (stream, _) = listener.accept().expect("accept test control connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set test socket timeout");
            let mut current = BufReader::new(stream);
            assert!(matches!(request(&mut current), Request::Hello { .. }));
            respond(
                &mut current,
                Response::Hello {
                    contract: CONTRACT,
                    instance: server_address.instance.clone(),
                    pid: server_address.pid,
                    build: serde_json::Value::Null,
                    accepting_starts: true,
                    mcp_registration: cccc_core::owned_mcp::REGISTRATION_API,
                },
            );
            assert!(matches!(request(&mut current), Request::Acquire { .. }));
            respond(&mut current, Response::Acquired { epoch: 2 });
            assert!(matches!(
                request(&mut current),
                Request::Mutate {
                    epoch: 2,
                    sequence: 1,
                    ..
                }
            ));
            respond(
                &mut current,
                Response::Admitted {
                    operation: OperationId {
                        instance: server_address.instance,
                        epoch: 2,
                        sequence: 1,
                    },
                },
            );
        });
        let client = Client {
            address,
            identified_source: false,
            same_source: false,
            control: Mutex::new(Some(Control {
                connection: Connection(BufReader::new(old_client)),
                epoch: 1,
                next_sequence: 0,
            })),
        };
        assert!(client.mutate(Mutation::StopAll).is_err());
        assert!(
            client
                .control
                .lock()
                .expect("fixture registry lock")
                .is_none()
        );
        let operation = client
            .mutate(Mutation::StopAll)
            .expect("submit owner mutation");
        assert_eq!((operation.epoch, operation.sequence), (2, 1));
        server.join().expect("test server thread completes");
    }
}
