#![cfg(unix)]
use cccc_core::workspace::{WriteOutcome, read_file, write_file};
use std::os::unix::{fs::symlink, net::UnixListener};
#[path = "support/workspace_fixture.rs"]
mod workspace_fixture;
use workspace_fixture::fixture;

#[test]
fn external_and_dangling_symlinks_are_rejected_without_disclosing_a_digest() {
    let f = fixture();
    let outside = tempfile::tempdir().expect("create outside-scope fixture");
    let secret = outside.path().join("secret");
    std::fs::write(&secret, "private bytes").expect("write outside-scope secret");
    symlink(&secret, f.repo.join("escape")).expect("create escaping symlink");
    assert!(write_file(&f.group, "escape", "replacement", "wrong").is_err());
    assert_eq!(
        std::fs::read_to_string(&secret).expect("read unchanged outside-scope secret"),
        "private bytes"
    );
    symlink(outside.path().join("missing"), f.repo.join("dangling"))
        .expect("create dangling symlink");
    assert!(write_file(&f.group, "dangling", "replacement", "").is_err());
    assert!(
        f.repo
            .join("dangling")
            .symlink_metadata()
            .expect("inspect dangling symlink")
            .is_symlink()
    );
}

#[test]
fn non_regular_targets_are_rejected_before_reading() {
    let f = fixture();
    let socket = f.repo.join("socket");
    let _listener = UnixListener::bind(&socket).expect("bind special-file socket");
    assert!(write_file(&f.group, "socket", "replacement", "").is_err());
    symlink("socket", f.repo.join("socket-link")).expect("create socket symlink");
    assert!(write_file(&f.group, "socket-link", "replacement", "").is_err());
    assert!(write_file(&f.group, "src", "replacement", "").is_err());
}

#[test]
fn internal_symlink_updates_the_validated_file_and_keeps_the_link() {
    let f = fixture();
    symlink("src/lib.rs", f.repo.join("alias.rs")).expect("create internal file symlink");
    let before = read_file(&f.group, "alias.rs").expect("read internal symlink target");
    assert!(matches!(
        write_file(&f.group, "alias.rs", "updated", &before.sha256)
            .expect("save internal symlink target"),
        WriteOutcome::Written { created: false, .. }
    ));
    assert_eq!(
        std::fs::read_to_string(f.repo.join("src/lib.rs")).expect("read saved target"),
        "updated"
    );
    assert!(
        f.repo
            .join("alias.rs")
            .symlink_metadata()
            .expect("inspect preserved symlink")
            .is_symlink()
    );
}

#[test]
fn fifo_is_rejected_without_blocking() {
    use std::process::Command;
    use std::time::{Duration, Instant};
    const CHILD: &str = "CCCC_TEST_FIFO_WRITE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let f = fixture();
        let fifo = f.repo.join("pipe");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("create FIFO")
                .success()
        );
        assert!(write_file(&f.group, "pipe", "replacement", "").is_err());
        symlink("pipe", f.repo.join("pipe-link")).expect("create FIFO symlink");
        assert!(write_file(&f.group, "pipe-link", "replacement", "").is_err());
        return;
    }
    let mut child = Command::new(std::env::current_exe().expect("locate test binary"))
        .args(["--exact", "fifo_is_rejected_without_blocking"])
        .env(CHILD, "1")
        .spawn()
        .expect("spawn isolated FIFO test");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().expect("poll FIFO test") {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill blocked FIFO test");
            child.wait().expect("reap blocked FIFO test");
            panic!("write_file blocked reading a FIFO");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
