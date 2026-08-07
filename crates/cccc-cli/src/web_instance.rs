use anyhow::{Context, Result};
use cccc_core::HomeLayout;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const LOCK_FILE: &str = "cccc-web.lock";

pub struct WebInstance {
    file: File,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RunningInstance {
    pub pid: Option<u32>,
}

pub enum Claim {
    Acquired(WebInstance),
    Running(RunningInstance),
}

pub fn try_claim(home: &HomeLayout) -> Result<Claim> {
    let path = lock_path(home);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open CCCC Web instance lock {}", path.display()))?;

    if file.try_lock_exclusive().is_err() {
        return Ok(Claim::Running(RunningInstance {
            pid: read_pid(&mut file),
        }));
    }

    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(file, "{}", std::process::id())?;
    file.flush()?;
    Ok(Claim::Acquired(WebInstance { file }))
}

pub fn confirm_stop(home: &HomeLayout, pid: Option<u32>) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    confirm_stop_with(home, pid, &mut io::stdin().lock(), &mut io::stderr().lock())
}

fn confirm_stop_with(
    home: &HomeLayout,
    pid: Option<u32>,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<bool> {
    let process = pid.map_or_else(|| "unknown PID".into(), |pid| format!("PID {pid}"));
    write!(
        output,
        "CCCC is already running for CCCC_HOME={} ({process}). Stop it and continue? [y/N] ",
        home.root().display()
    )?;
    output.flush()?;

    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn read_pid(file: &mut File) -> Option<u32> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut value = String::new();
    file.read_to_string(&mut value).ok()?;
    value.trim().parse().ok()
}

fn lock_path(home: &HomeLayout) -> PathBuf {
    home.daemon_dir().join(LOCK_FILE)
}

impl Drop for WebInstance {
    fn drop(&mut self) {
        let _ = self.file.set_len(0);
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_scoped_to_home_and_reports_owner_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = HomeLayout::from_path(temp.path().join("first")).expect("first home");
        let second = HomeLayout::from_path(temp.path().join("second")).expect("second home");
        first.initialize().expect("initialize first");
        second.initialize().expect("initialize second");

        let first_claim = try_claim(&first).expect("claim first");
        assert!(matches!(first_claim, Claim::Acquired(_)));
        assert!(matches!(
            try_claim(&first).expect("detect first"),
            Claim::Running(RunningInstance { pid: Some(pid) }) if pid == std::process::id()
        ));
        assert!(matches!(
            try_claim(&second).expect("claim second"),
            Claim::Acquired(_)
        ));
    }

    #[test]
    fn accepts_only_explicit_yes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut output = Vec::new();

        assert!(
            confirm_stop_with(&home, Some(42), &mut "yes\n".as_bytes(), &mut output).expect("yes")
        );
        assert!(
            !confirm_stop_with(&home, Some(42), &mut "\n".as_bytes(), &mut output)
                .expect("default no")
        );
    }

    #[test]
    fn reclaims_an_unlocked_stale_pid_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        std::fs::write(lock_path(&home), "10740\n").expect("write stale pid");

        let claim = try_claim(&home).expect("reclaim stale lock");
        assert!(matches!(claim, Claim::Acquired(_)));
        assert_eq!(
            std::fs::read_to_string(lock_path(&home))
                .expect("read current pid")
                .trim(),
            std::process::id().to_string()
        );
    }
}
