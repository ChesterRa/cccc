use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn supports_hooks(
    command: &[String],
    cccc_executable: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let mut probe_command = vec![program.clone()];
    probe_command.extend(super::overrides::hook_arguments(cccc_executable));
    probe_command.extend(["mcp".into(), "list".into()]);
    let probe_command = cccc_runtime::prepare_pty_command(&probe_command, env);
    let Some((program, args)) = probe_command.split_first() else {
        return false;
    };
    let mut probe = Command::new(program);
    probe
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut child) = probe.spawn() else {
        return false;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}
