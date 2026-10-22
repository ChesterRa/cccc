use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Process {
    pub pid: u32,
    pub parent: u32,
    pub start: u64,
    pub namespace: u64,
    // Upper bound on process birth, including one kernel clock tick.
    pub born_before: f64,
}

pub(super) trait Processes {
    fn read(&self, pid: u32) -> Result<Process, &'static str>;
}

pub(super) struct ProcFs;

#[cfg(target_os = "linux")]
impl Processes for ProcFs {
    fn read(&self, pid: u32) -> Result<Process, &'static str> {
        use nix::time::{ClockId, clock_gettime};
        use nix::unistd::{SysconfVar, sysconf};
        let base = PathBuf::from(format!("/proc/{pid}"));
        let stat = std::fs::read_to_string(base.join("stat")).map_err(|_| "process_unavailable")?;
        let (_, fields) = stat.rsplit_once(')').ok_or("process_invalid")?;
        let fields = fields.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 20 || matches!(fields[0], "Z" | "X") {
            return Err("process_not_live");
        }
        let parent = fields[1].parse().map_err(|_| "process_invalid")?;
        let start: u64 = fields[19].parse().map_err(|_| "process_invalid")?;
        let namespace =
            std::fs::read_link(base.join("ns/pid")).map_err(|_| "namespace_unavailable")?;
        let namespace = namespace
            .to_str()
            .and_then(|s| s.strip_prefix("pid:["))
            .and_then(|s| s.strip_suffix(']'))
            .and_then(|s| s.parse().ok())
            .ok_or("namespace_invalid")?;
        let hz = sysconf(SysconfVar::CLK_TCK)
            .map_err(|_| "clock_unavailable")?
            .filter(|hz| *hz > 0)
            .ok_or("clock_unavailable")? as f64;
        let boot = clock_gettime(ClockId::CLOCK_BOOTTIME).map_err(|_| "clock_unavailable")?;
        let real = clock_gettime(ClockId::CLOCK_REALTIME).map_err(|_| "clock_unavailable")?;
        let seconds = |ts: nix::sys::time::TimeSpec| ts.tv_sec() as f64 + ts.tv_nsec() as f64 / 1e9;
        Ok(Process {
            pid,
            parent,
            start,
            namespace,
            born_before: seconds(real) - seconds(boot) + (start as f64 + 1.0) / hz,
        })
    }
}

#[cfg(not(target_os = "linux"))]
impl Processes for ProcFs {
    fn read(&self, _pid: u32) -> Result<Process, &'static str> {
        Err("process_identity_unsupported")
    }
}

pub(super) fn owned(
    processes: &impl Processes,
    root: &Process,
    holder: u32,
) -> Result<Option<Process>, &'static str> {
    let original = processes.read(holder)?;
    let mut current = original.clone();
    // A bounded ancestry walk, never a scan of the machine's process table.
    for _ in 0..64 {
        if current.pid == root.pid {
            return if current.start == root.start && current.namespace == root.namespace {
                Ok(Some(original))
            } else {
                Err("generation_changed")
            };
        }
        if current.parent <= 1 {
            return Ok(None);
        }
        current = processes.read(current.parent)?;
    }
    Err("process_tree_limit")
}

#[cfg(target_os = "linux")]
pub(super) fn provider_home(pid: u32, group: &str, actor: &str) -> Result<PathBuf, &'static str> {
    use std::io::Read;
    let file = std::fs::File::open(format!("/proc/{pid}/environ"))
        .map_err(|_| "environment_unavailable")?;
    let mut bytes = Vec::new();
    file.take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|_| "environment_unavailable")?;
    if bytes.len() > 131_072 {
        return Err("environment_limit");
    }
    // Only identity/location keys escape this function; never log environment data.
    let find = |name: &[u8]| {
        bytes.split(|b| *b == 0).find_map(|entry| {
            let (key, value) = entry.split_at(entry.iter().position(|b| *b == b'=')?);
            (key == name)
                .then(|| std::str::from_utf8(&value[1..]).ok())
                .flatten()
        })
    };
    if find(b"CCCC_GROUP_ID") != Some(group) || find(b"CCCC_ACTOR_ID") != Some(actor) {
        return Err("actor_identity_mismatch");
    }
    // Named profiles can relocate the database. They require a separately verified adapter.
    if find(b"HERMES_PROFILE").is_some_and(|s| !s.is_empty() && s != "default") {
        return Err("profile_unsupported");
    }
    let home = if let Some(home) = find(b"HERMES_HOME").filter(|s| !s.is_empty()) {
        PathBuf::from(home)
    } else {
        PathBuf::from(find(b"HOME").ok_or("home_unavailable")?).join(".hermes")
    };
    if !home.is_absolute() {
        return Err("home_not_absolute");
    }
    Ok(home)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn provider_home(
    _pid: u32,
    _group: &str,
    _actor: &str,
) -> Result<PathBuf, &'static str> {
    Err("process_identity_unsupported")
}
