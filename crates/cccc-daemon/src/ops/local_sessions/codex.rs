use super::transport::Outcome;
use cccc_core::{HomeLayout, local_sessions};
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::{runtime::Runtime, sync::Semaphore};

// Native I/O shares a runtime instead of building one for every message. A
// single qualification slot bounds best-effort post-failure probes; no polling
// or automatic positive requalification is introduced.
static RUNTIME: OnceLock<io::Result<Runtime>> = OnceLock::new();
static QUALIFICATION: Semaphore = Semaphore::const_new(1);

fn runtime() -> io::Result<&'static Runtime> {
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("cccc-native-io")
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|error| io::Error::new(error.kind(), "Native I/O runtime unavailable"))
}

pub(super) fn refresh(
    home: &HomeLayout,
    session: &local_sessions::NativeSession,
) -> io::Result<()> {
    runtime()?.block_on(async {
        let _permit = QUALIFICATION
            .acquire()
            .await
            .map_err(|_| io::Error::other("Native qualification unavailable"))?;
        recheck(home.clone(), session.clone(), true).await
    })
}

async fn recheck(
    home: HomeLayout,
    session: local_sessions::NativeSession,
    restore: bool,
) -> io::Result<()> {
    let ready = match (&session.codex_program, &session.codex_home) {
        (Some(program), Some(codex_home)) => {
            cccc_runtime::native_codex_can_receive(
                program,
                codex_home,
                std::path::Path::new(&session.cwd),
                &session.native_session_id,
            )
            .await?
        }
        _ => false,
    };
    if !ready || restore {
        tokio::task::spawn_blocking(move || {
            local_sessions::set_codex_receive_ready(&home, &session, ready)
        })
        .await
        .map_err(|_| io::Error::other("Native readiness update unavailable"))??;
    }
    Ok(())
}

fn ready_session(
    home: &HomeLayout,
    source_id: &str,
) -> Result<local_sessions::NativeSession, super::PreflightError> {
    local_sessions::read(home, source_id)
        .ok()
        .filter(|session| session.host.alive() && session.codex_can_receive)
        .ok_or(super::PreflightError::Unavailable)
}

pub(super) fn preflight(
    home: &HomeLayout,
    source_id: &str,
    text: &str,
) -> Result<(), super::PreflightError> {
    queue_command(&ready_session(home, source_id)?, text).map(|_| ())
}

fn queue_command(
    session: &local_sessions::NativeSession,
    text: &str,
) -> Result<std::process::Command, super::PreflightError> {
    use super::PreflightError::{Rejected, Unavailable};
    let (Some(program), Some(codex_home)) = (
        session.codex_program.as_deref(),
        session.codex_home.as_deref(),
    ) else {
        return Err(Unavailable);
    };
    let args = [
        "queue",
        "--remote",
        "unix://",
        "--thread",
        &session.native_session_id,
        "--message",
        text,
    ];
    // Check precisely the argv we will pass, after receiving context, bootstrap
    // and message rendering. No stdin protocol, truncation or implicit splitting.
    if program.as_os_str().as_encoded_bytes().contains(&0)
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err(Rejected("Native Codex command-line cannot contain NUL characters; this delivery was not submitted".into()));
    }
    #[cfg(unix)]
    {
        use nix::unistd::{SysconfVar, sysconf};
        let limit = |name| {
            sysconf(name)
                .ok()
                .flatten()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or(Unavailable)
        };
        // argv alone must fit even with an empty environment. Environment or
        // other transient OS pressure is not a permanent message rejection;
        // actual no-child spawn failures still carry retryable evidence.
        let argv = std::iter::once(program.as_os_str().as_encoded_bytes().len() + 1)
            .chain(args.iter().map(|arg| arg.len() + 1));
        let too_large = argv.clone().sum::<usize>() > limit(SysconfVar::ARG_MAX)?;
        #[cfg(target_os = "linux")]
        let too_large = {
            let argument_limit = limit(SysconfVar::PAGE_SIZE)?.saturating_mul(32);
            too_large || argv.clone().any(|bytes| bytes > argument_limit)
        };
        if too_large {
            return Err(Rejected("Native Codex command-line size limit exceeded by the rendered delivery; shorten it or use an attachment; this delivery was not submitted".into()));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let batch = program
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            });
        check_windows_command_line(program.as_os_str().encode_wide().count(), &args, batch)
            .map_err(Rejected)?;
    }
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .env("CODEX_HOME", codex_home)
        .current_dir(&session.cwd);
    Ok(command)
}

// Match std::Command's regular-argument encoding (Rust 1.88 Windows args.rs),
// including escaped quotes/backslashes and UTF-16 surrogate pairs. Batch shims
// have a separate cmd.exe limit and reject CR/LF before CreateProcess; rendered
// native payloads contain newlines, so users must select the native executable.
#[cfg(any(windows, test))]
fn check_windows_command_line(
    program_units: usize,
    args: &[&str],
    batch: bool,
) -> Result<(), String> {
    if batch && args.iter().any(|arg| arg.contains(['\r', '\n'])) {
        return Err("Native Codex Windows batch command-line cannot carry a multiline delivery; select the native Codex executable; this delivery was not submitted".into());
    }
    let prefix = if batch {
        "cmd.exe /e:ON /v:OFF /d /c \"".len() + 1
    } else {
        0
    };
    // Both paths quote argv[0]; include the final NUL, spaces and batch wrapper.
    let units = program_units
        + 3
        + prefix
        + args
            .iter()
            .map(|arg| 1 + windows_argument_len(arg, batch))
            .sum::<usize>();
    let limit = if batch { 8_191 } else { 32_767 };
    if units > limit {
        return Err("Native Codex Windows command-line size limit exceeded by the rendered delivery; shorten it or use an attachment; this delivery was not submitted".into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn windows_argument_len(arg: &str, batch: bool) -> usize {
    let quote = if batch {
        arg.is_empty()
            || arg.ends_with('\\')
            || arg.chars().any(|c| {
                c.is_control()
                    || (c.is_ascii() && !(c.is_ascii_alphanumeric() || "#$*+-./:?@\\_".contains(c)))
            })
    } else {
        arg.is_empty() || arg.contains([' ', '\t'])
    };
    let mut units = arg.encode_utf16().count() + 2 * usize::from(quote);
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                units += backslashes + 1;
            }
            if batch && c == '%' {
                units += 7;
            }
            backslashes = 0;
        }
    }
    if quote {
        units += backslashes;
    }
    units
}

pub(super) fn submit(
    home: &HomeLayout,
    source_id: &str,
    text: &str,
    cancelled: &AtomicBool,
) -> Outcome {
    let session = match ready_session(home, source_id) {
        Ok(session) => session,
        _ => return Outcome::Unavailable,
    };
    if cancelled.load(Ordering::Acquire) {
        return Outcome::Unavailable;
    }
    let mut command = match queue_command(&session, text) {
        Ok(command) => command,
        Err(super::PreflightError::Unavailable) => return Outcome::Unavailable,
        Err(super::PreflightError::Rejected(reason)) => return Outcome::Rejected(reason),
    };
    // An explicit endpoint prevents the queue client from starting another
    // native daemon after the original host has exited.
    let run = async {
        if let (Some(program), Some(codex_home)) = (
            session.codex_program.as_deref(),
            session.codex_home.as_deref(),
        ) {
            match cccc_runtime::native_codex_try_steer(
                program,
                codex_home,
                std::path::Path::new(&session.cwd),
                &session.native_session_id,
                text,
                cancelled,
            )
            .await
            {
                cccc_runtime::NativeCodexSteerOutcome::Accepted => return Outcome::Accepted,
                cccc_runtime::NativeCodexSteerOutcome::Ambiguous => {
                    return Outcome::Ambiguous(
                        "Native Codex did not confirm this handoff; it was not automatically repeated"
                            .into(),
                    );
                }
                cccc_runtime::NativeCodexSteerOutcome::NotSubmitted => {}
            }
        }
        if cancelled.load(Ordering::Acquire) {
            return Outcome::Unavailable;
        }
        let capture = cccc_runtime::capture_command_with_start_evidence(
            &mut command,
            None,
            Duration::from_secs(10),
            4096,
        );
        tokio::pin!(capture);
        let output = loop {
            tokio::select! {
                biased;
                // Poll capture first: a returned no-child error is authoritative;
                // cancellation of an admitted/polled capture is not that proof.
                output=&mut capture=>break output,
                ()=tokio::time::sleep(Duration::from_millis(50))=>{
                    if cancelled.load(Ordering::Acquire){break Err(cccc_runtime::CommandError::Started(std::io::Error::new(std::io::ErrorKind::Interrupted,"Native handoff cancelled")))}
                }
            }
        };
        match output {
            Ok(output) if output.status.success() => Outcome::Accepted,
            Ok(_) => Outcome::Rejected(
                "Native Codex rejected this handoff; check the original session".into(),
            ),
            Err(cccc_runtime::CommandError::NotStarted(_)) => Outcome::Unavailable,
            Err(cccc_runtime::CommandError::Started(_)) => Outcome::Ambiguous(
                "Native Codex did not confirm this handoff; it was not automatically repeated"
                    .into(),
            ),
        }
    };
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(_) => return Outcome::Unavailable,
    };
    let outcome = runtime.block_on(run);
    if matches!(outcome, Outcome::Rejected(_) | Outcome::Ambiguous(_))
        && !cancelled.load(Ordering::Acquire)
        && let Ok(permit) = QUALIFICATION.try_acquire()
    {
        let home = home.clone();
        runtime.spawn(async move {
            let _permit = permit;
            // This read-only follow-up must not delay the delivery receipt or
            // actor shutdown. Unknown evidence leaves the snapshot untouched.
            let _ = recheck(home, session, false).await;
        });
    }
    // A business rejection with a qualified channel, or an inconclusive lookup,
    // does not disconnect the source. Neither lookup changes this attempt's
    // failed/ambiguous result or authorizes replay of its input.
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_preflight_counts_quoted_utf16_and_the_complete_command_line() {
        for (arg, expected) in [
            ("", 2),
            ("abc", 3),
            ("a b", 5),
            ("界😀", 3),
            ("a\"b", 4),
            ("a\\\"b", 6),
            ("a b\\", 7),
        ] {
            assert_eq!(windows_argument_len(arg, false), expected, "{arg:?}");
        }
        let program = "C:\\工具 目录\\codex.exe".encode_utf16().count();
        let fixed = [
            "queue",
            "--remote",
            "unix://",
            "--thread",
            "thread-fixture",
            "--message",
        ];
        let overhead = program
            + 3
            + fixed
                .iter()
                .map(|arg| 1 + windows_argument_len(arg, false))
                .sum::<usize>()
            + 1;
        let exact = "x".repeat(32_767 - overhead);
        let mut args = fixed.to_vec();
        args.push(&exact);
        assert!(check_windows_command_line(program, &args, false).is_ok());
        let too_long = format!("{exact}x");
        args[6] = &too_long;
        assert!(check_windows_command_line(program, &args, false).is_err());
        let quotes = "\"".repeat(17_000);
        args[6] = &quotes;
        assert!(quotes.chars().count() < 32_767);
        assert!(check_windows_command_line(program, &args, false).is_err());
        let supplementary = "😀".repeat(17_000);
        args[6] = &supplementary;
        assert!(check_windows_command_line(program, &args, false).is_err());
        println!(
            "Windows calculation: full quoted UTF-16 limit includes {overhead} fixed units; exact boundary accepted, +1 / escaped quotes / surrogate pairs rejected"
        );
    }

    #[test]
    fn windows_batch_preflight_rejects_std_unrepresentable_input_and_counts_expansion() {
        assert_eq!(windows_argument_len("%", true), 10);
        assert_eq!(windows_argument_len("end\\", true), 7);
        assert!(
            check_windows_command_line(20, &["queue", "line 1\nline 2"], true)
                .expect_err("std rejects batch CR/LF")
                .contains("multiline")
        );
        assert!(check_windows_command_line(20, &[&"%".repeat(1_100)], true).is_err());
        assert!(check_windows_command_line(20, &[&"x".repeat(8_192)], true).is_err());
    }
}
