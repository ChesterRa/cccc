use super::super::TranscriptFollower;
use serde_json::json;
use std::io::{self, Write};
use std::path::PathBuf;

const ID: &str = "52b41c61-e23c-4b7c-8b60-809c347451b5";

struct Fixture {
    _temp: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
    old: PathBuf,
    actual: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create test directory");
        let config = temp.path().to_path_buf();
        let old = config.join("projects/main").join(format!("{ID}.jsonl"));
        let actual = config
            .join("projects/main-worktree")
            .join(format!("{ID}.jsonl"));
        std::fs::create_dir_all(old.parent().expect("access fixture parent directory"))
            .expect("create fixture directory");
        std::fs::create_dir_all(actual.parent().expect("access fixture parent directory"))
            .expect("create fixture directory");
        std::fs::write(
            &actual,
            format!(
                "{}\n",
                json!({"type":"user","message":{"content":"old work"}})
            ),
        )
        .expect("write test fixture");
        let state = config.join("state.json");
        std::fs::write(&state, json!({"linkScanPath":old}).to_string())
            .expect("write test fixture");
        Self {
            _temp: temp,
            config,
            state,
            old,
            actual,
        }
    }

    fn follower(&self) -> TranscriptFollower {
        TranscriptFollower::new(self.state.clone(), self.config.clone(), ID.into(), true)
    }
}

#[tokio::test]
async fn stale_main_project_pointer_recovers_worktree_history_and_fences_old_messages() {
    let fixture = Fixture::new();
    let mut follower = fixture.follower();
    follower
        .initialize()
        .await
        .expect("recover worktree transcript");
    assert!(
        follower
            .poll()
            .await
            .expect("complete poll in fixture")
            .is_empty(),
        "old messages must not replay"
    );
    let next = json!({"type":"user","message":{"content":"new work"}});
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&fixture.actual)
        .expect("complete open in fixture");
    writeln!(file, "{next}").expect("fixture value must be available");
    assert_eq!(
        follower.poll().await.expect("complete poll in fixture"),
        vec![next]
    );
    // Agent View may keep the stale pointer for several polls, then correct it.
    assert!(
        follower
            .poll()
            .await
            .expect("complete poll in fixture")
            .is_empty()
    );
    std::fs::write(
        &fixture.state,
        json!({"linkScanPath":fixture.actual}).to_string(),
    )
    .expect("write test fixture");
    assert!(
        follower
            .poll()
            .await
            .expect("complete poll in fixture")
            .is_empty()
    );
    assert!(
        !fixture.old.exists(),
        "recovery must not fabricate or overwrite transcripts"
    );
}

#[tokio::test]
async fn refuses_to_follow_a_second_move_after_initialization() {
    let fixture = Fixture::new();
    let mut follower = fixture.follower();
    follower
        .initialize()
        .await
        .expect("complete initialize in fixture");
    std::fs::rename(
        &fixture.actual,
        fixture
            .old
            .parent()
            .expect("access fixture parent directory")
            .join("moved.jsonl"),
    )
    .expect("complete rename in fixture");
    assert_eq!(
        follower
            .poll()
            .await
            .expect_err("operation must fail in this scenario")
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[tokio::test]
async fn refuses_recreated_stale_path_and_same_path_replacement() {
    for recreate_old in [true, false] {
        let fixture = Fixture::new();
        let mut follower = fixture.follower();
        follower
            .initialize()
            .await
            .expect("complete initialize in fixture");
        let target = if recreate_old {
            &fixture.old
        } else {
            std::fs::rename(&fixture.actual, fixture.actual.with_extension("backup"))
                .expect("complete rename in fixture");
            &fixture.actual
        };
        std::fs::write(target, "{}\n").expect("write test fixture");
        assert_eq!(
            follower
                .poll()
                .await
                .expect_err("operation must fail in this scenario")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[tokio::test]
async fn refuses_ambiguous_or_wrong_session_transcripts() {
    let fixture = Fixture::new();
    let duplicate = fixture.config.join("projects/duplicate");
    std::fs::create_dir_all(&duplicate).expect("create fixture directory");
    std::fs::copy(&fixture.actual, duplicate.join(format!("{ID}.jsonl")))
        .expect("complete copy in fixture");
    assert_eq!(
        fixture
            .follower()
            .discover_path()
            .await
            .expect_err("operation must fail in this scenario")
            .kind(),
        io::ErrorKind::InvalidData
    );
    std::fs::write(
        &fixture.state,
        json!({"linkScanPath":fixture.old.with_file_name("another-session.jsonl")}).to_string(),
    )
    .expect("write test fixture");
    assert_eq!(
        fixture
            .follower()
            .discover_path()
            .await
            .expect_err("operation must fail in this scenario")
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_symlinks_instead_of_hiding_them_with_relocation() {
    for published_link in [true, false] {
        let fixture = Fixture::new();
        if published_link {
            std::os::unix::fs::symlink(&fixture.actual, &fixture.old)
                .expect("complete symlink in fixture");
        } else {
            let outside = fixture.config.join("outside.jsonl");
            std::fs::rename(&fixture.actual, &outside).expect("complete rename in fixture");
            std::os::unix::fs::symlink(outside, &fixture.actual)
                .expect("complete symlink in fixture");
        }
        assert_eq!(
            fixture
                .follower()
                .discover_path()
                .await
                .expect_err("operation must fail in this scenario")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[tokio::test]
async fn reports_missing_path_without_switching_a_fresh_session() {
    let fixture = Fixture::new();
    let mut fresh = TranscriptFollower::new(
        fixture.state.clone(),
        fixture.config.clone(),
        ID.into(),
        false,
    );
    let error = fresh
        .discover_path()
        .await
        .expect_err("operation must fail in this scenario");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(
        error
            .to_string()
            .contains(&fixture.old.display().to_string())
    );
}

#[tokio::test]
async fn missing_history_reports_the_session_and_does_not_adopt_another_session() {
    let fixture = Fixture::new();
    std::fs::rename(
        &fixture.actual,
        fixture.actual.with_file_name("another-session.jsonl"),
    )
    .expect("complete rename in fixture");
    let error = fixture
        .follower()
        .discover_path()
        .await
        .expect_err("operation must fail in this scenario");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains(ID));
    assert!(
        error
            .to_string()
            .contains(&fixture.old.display().to_string())
    );
    assert!(!fixture.actual.exists());
}
