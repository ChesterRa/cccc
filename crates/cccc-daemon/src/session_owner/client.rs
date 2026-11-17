use super::protocol::{
    ActorSnapshot, CONTRACT, ExecutionFact, Failure, MAX_FRAME_BYTES, Mutation, OperationId,
    OperationSource, OperationState, Request, Response,
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
    control: Mutex<Option<Control>>,
}

struct Control {
    connection: Connection,
    epoch: u64,
    next_sequence: u64,
}

struct Connection(BufReader<UnixStream>);

impl Client {
    pub(super) fn connect(address: Address) -> io::Result<Self> {
        let mut connection = Connection::connect(&address)?;
        connection.verify(&address)?;
        Ok(Self {
            address,
            control: Mutex::new(None),
        })
    }

    /// A read never acquires or replaces the mutating stream.
    pub(super) fn accepting_starts(&self) -> io::Result<bool> {
        match self.read(Request::Hello { contract: CONTRACT })? {
            Response::Hello {
                accepting_starts, ..
            } => Ok(accepting_starts),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn snapshots(&self) -> io::Result<Vec<ActorSnapshot>> {
        match self.read(Request::Snapshot)? {
            Response::Snapshot { mut actors } => {
                for actor in &mut actors {
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
                Ok(actors)
            }
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
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
        match self.change(mutation)? {
            Response::Admitted { operation } => Ok(operation),
            Response::Failed { error } => Err(failure(error)),
            _ => Err(invalid_response()),
        }
    }

    pub(super) fn submit(
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

impl Connection {
    fn connect(address: &Address) -> io::Result<Self> {
        let socket = (|| {
            let path = SocketPath::new(&address.socket)?;
            UnixStream::connect(&path.path)
        })()
        .map_err(|error| with_owner(address, error))?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self(BufReader::new(socket)))
    }

    fn verify(&mut self, address: &Address) -> io::Result<()> {
        match self
            .exchange(&Request::Hello { contract: CONTRACT })
            .map_err(|error| with_owner(address, error))?
        {
            Response::Hello {
                contract,
                instance,
                pid,
                ..
            } if contract == CONTRACT && instance == address.instance && pid == address.pid => {
                Ok(())
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
            "the embedding host must provide a cccc session-owner entry",
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
            && let Ok(client) = Client::connect(address)
        {
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
    io::Error::new(
        error.kind(),
        format!(
            "session owner {} (PID {}, instance {}) could not be verified ({:?}); agents may still be running; inspect that process and retry `cccc daemon stop` after it responds",
            address.owner_id,
            address.pid,
            address.instance,
            error.kind()
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
