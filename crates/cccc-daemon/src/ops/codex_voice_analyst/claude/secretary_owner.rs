//! Task-owned Agent View jobs need control cleanup, not process-group inference.
use super::*;
use cccc_core::HomeLayout;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Owner {
    config_dir: PathBuf,
    settings_path: PathBuf,
}

pub(super) fn path(cwd: &Path) -> io::Result<PathBuf> {
    Ok(cwd
        .parent()
        .ok_or_else(|| io::Error::other("Secretary task directory is missing"))?
        .join("claude-owner.json"))
}

pub(super) fn record(cwd: &Path, prepared: &command::PreparedClaude) -> io::Result<()> {
    cccc_core::fs::write_json(
        &path(cwd)?,
        &Owner {
            config_dir: prepared.config_dir.clone(),
            settings_path: prepared.settings_path.clone(),
        },
    )
}

pub(super) async fn cleanup(home: &HomeLayout, cwd: &Path) -> io::Result<()> {
    let path = path(cwd)?;
    let owner: Owner = match cccc_core::fs::read_json(&path) {
        Ok(owner) => owner,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let settings_parent = owner
        .settings_path
        .parent()
        .ok_or_else(|| io::Error::other("Secretary settings owner is invalid"))?
        .canonicalize()?;
    if settings_parent != home.daemon_dir().join("claude-managed").canonicalize()? {
        return Err(io::Error::other("Secretary settings owner is invalid"));
    }
    let canonical_cwd = cwd.canonicalize()?;
    let matches_cwd = |value: &Value| {
        value["cwd"].as_str().is_some_and(|value| {
            Path::new(value) == cwd
                || Path::new(value)
                    .canonicalize()
                    .is_ok_and(|path| path == canonical_cwd)
        })
    };
    // The task working directory is unique and is never used by an Actor or
    // Analyst. Only records with this exact cwd can be stopped.
    match control::Endpoint::resolve(&owner.config_dir) {
        Ok(endpoint) => {
            for value in control::list(&endpoint).await? {
                if !matches_cwd(&value) {
                    continue;
                }
                let job = parse_job_required(&value)?;
                validate_worker_version(&job)?;
                kill_and_confirm(&endpoint, &job.short).await?;
            }
        }
        Err(error) if retryable_control_error(&error) => {
            // A rejected launch may never create a supervisor. Durable job
            // records still prevent declaring an inaccessible worker cleaned.
            let jobs = owner.config_dir.join("jobs");
            if jobs.is_dir() {
                for entry in std::fs::read_dir(jobs)? {
                    let entry = entry?;
                    if !entry.file_type()?.is_dir() {
                        continue;
                    }
                    let state: Value = cccc_core::fs::read_json(&entry.path().join("state.json"))?;
                    if matches_cwd(&state) {
                        return Err(io::Error::other(
                            "Secretary Agent View cleanup requires its control service",
                        ));
                    }
                }
            }
        }
        Err(error) => return Err(error),
    }
    match std::fs::remove_file(&owner.settings_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::remove_file(path)
}
