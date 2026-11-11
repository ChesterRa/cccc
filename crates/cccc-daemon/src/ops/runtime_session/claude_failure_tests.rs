use super::*;

const SESSION: &str = "52b41c61-e23c-4b7c-8b60-809c347451b5";

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: String,
    cwd: std::path::PathBuf,
    command: Vec<String>,
    env: BTreeMap<String, String>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("Claude resume failure", "")
            .expect("group")
            .group_id;
        let cwd = temp.path().join("workspace");
        std::fs::create_dir(&cwd).expect("workspace");
        let fixture = Self {
            _temp: temp,
            home,
            group,
            cwd,
            command: vec!["claude".into()],
            env: BTreeMap::new(),
        };
        fixture.success();
        fixture
    }

    fn prepare(&self) -> io::Result<Option<ResumeAttempt>> {
        prepare_managed(
            &self.home,
            &self.group,
            "worker",
            &self.cwd,
            &self.command,
            &self.env,
        )
    }

    fn success(&self) {
        record_managed(
            &self.home,
            &self.group,
            "worker",
            &self.cwd,
            &self.command,
            &self.env,
            SESSION,
            true,
        )
        .expect("record success");
    }

    fn failure(&self, attempt: &ResumeAttempt, blocked: bool) -> Option<String> {
        record_resume_failure(
            &self.home,
            &self.group,
            "worker",
            attempt,
            "Safe fixture diagnosis",
            blocked,
        )
        .expect("record failure")
    }

    fn receipt(&self) -> Map<String, Value> {
        read(&self.home, &self.group, "worker").expect("receipt")
    }
}

#[test]
fn failed_resume_preserves_identity_and_pauses_automatic_launch_without_new_session() {
    let f = Fixture::new();
    let attempt = f.prepare().expect("prepare").expect("saved session");
    f.failure(&attempt, true);
    let failed = f.receipt();
    assert_eq!(failed["provider_session_id"], SESSION);
    assert_eq!(failed["failure_count"], 1);
    assert_eq!(failed["status"], "resume_failed");
    assert_eq!(failed["resume_eligible"], false);
    for _ in 0..4 {
        let error = f
            .prepare()
            .err()
            .expect("automatic launch is blocked, not fresh");
        assert!(is_resume_blocked(&error));
    }
    assert_eq!(
        f.receipt(),
        failed,
        "blocked reads must not manufacture attempts"
    );
    assert!(retry_failed_resume(&f.home, &f.group, "worker").expect("explicit retry"));
    let retry = f.prepare().expect("retry").expect("same saved identity");
    assert_eq!(retry.session_id, SESSION);
    assert_ne!(retry.attempt_id, attempt.attempt_id);
    f.failure(&retry, true);
    assert_eq!(f.receipt()["failure_count"], 2);
}

#[test]
fn only_a_known_path_change_selects_cold_resume_and_preserves_the_other_fences() {
    let mut f = Fixture::new();
    f.env.insert("PATH".into(), "/before/bin:/usr/bin".into());
    f.success();
    let saved = f.receipt();
    assert!(!f.prepare().expect("same launch").expect("resume").cold_only);
    f.env.insert("PATH".into(), "/after/bin:/usr/bin".into());
    let attempt = f.prepare().expect("PATH changed").expect("cold resume");
    assert_eq!(attempt.session_id, SESSION);
    assert!(attempt.cold_only);

    for field in ["CCCC_CLI", "CLAUDE_CONFIG_DIR", "PROVIDER_SETTING"] {
        f.env.insert(field.into(), "/other/value".into());
        assert!(
            f.prepare().expect("another setting changed").is_none(),
            "{field}"
        );
        f.env.remove(field);
    }
    for (name, parts) in [
        ("missing", None),
        (
            "unknown version",
            Some(json!({"v":99,"path":"old","cccc_cli":"a","rest":"b"})),
        ),
        ("incomplete", Some(json!({"v":1,"path":"old"}))),
        (
            "wrong type",
            Some(json!({"v":1,"path":false,"cccc_cli":"a","rest":"b"})),
        ),
        (
            "does not explain the changed total",
            Some(json!(
                launch_identity(&f.command, &f.env, &f.cwd)
                    .expect("identity")
                    .1
            )),
        ),
    ] {
        let mut receipt = saved.clone();
        if let Some(parts) = parts {
            receipt.insert("launch_parts".into(), parts);
        } else {
            receipt.remove("launch_parts");
        }
        write(&f.home, &f.group, "worker", &receipt).expect("stored receipt");
        assert!(f.prepare().expect("unsupported split").is_none(), "{name}");
    }
    for fingerprint in [Value::Null, json!(""), json!(false)] {
        let mut receipt = saved.clone();
        receipt.insert("identity_fingerprint".into(), fingerprint);
        write(&f.home, &f.group, "worker", &receipt).expect("incomplete receipt");
        assert!(f.prepare().expect("missing original fingerprint").is_none());
    }
    write(&f.home, &f.group, "worker", &saved).expect("original receipt");
    f.command.extend(["--model".into(), "other".into()]);
    assert!(f.prepare().expect("changed model/command").is_none());
    f.command.truncate(1);
    let elsewhere = f.cwd.join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("other workspace");
    assert!(
        prepare_managed(&f.home, &f.group, "worker", &elsewhere, &f.command, &f.env)
            .expect("changed workspace")
            .is_none()
    );

    // A PATH allowance is not permission to bypass a previously paused resume.
    let attempt = f
        .prepare()
        .expect("prepare before pause")
        .expect("cold resume");
    f.failure(&attempt, true);
    assert!(is_resume_blocked(
        &f.prepare().err().expect("paused resume")
    ));
}

#[test]
fn transient_resume_failure_retains_eligibility_and_success_clears_diagnostics() {
    let f = Fixture::new();
    let attempt = f.prepare().expect("prepare").expect("saved session");
    f.failure(&attempt, false);
    assert_eq!(f.receipt()["status"], "usable");
    assert_eq!(f.receipt()["resume_eligible"], true);
    assert_eq!(f.receipt()["failure_count"], 1);
    assert!(
        !f.receipt()["last_resume_error"]
            .as_str()
            .expect("diagnostic")
            .is_empty()
    );
    assert_eq!(
        f.prepare().expect("retry").expect("session").session_id,
        SESSION
    );
    f.success();
    assert_eq!(f.receipt()["failure_count"], 0);
    assert_eq!(f.receipt()["last_resume_error"], "");
}

#[test]
fn repeated_transient_resume_failures_pause_automatic_launch_but_keep_the_conversation() {
    let f = Fixture::new();
    for failure in 1..AUTOMATIC_RESUME_FAILURE_LIMIT {
        let attempt = f.prepare().expect("prepare").expect("saved session");
        assert_eq!(
            f.failure(&attempt, false),
            None,
            "failure {failure} retries"
        );
        assert_eq!(f.receipt()["status"], "usable");
    }
    let attempt = f.prepare().expect("prepare").expect("saved session");
    let paused = f
        .failure(&attempt, false)
        .expect("the limit pauses recovery");
    let failed = f.receipt();
    assert_eq!(failed["status"], "resume_failed");
    assert_eq!(failed["resume_eligible"], true);
    assert_eq!(failed["provider_session_id"], SESSION);
    assert_eq!(failed["failure_count"], AUTOMATIC_RESUME_FAILURE_LIMIT);
    assert_eq!(failed["last_resume_error"], paused.as_str());
    assert!(paused.starts_with("Safe fixture diagnosis "), "{paused}");
    assert!(
        paused.contains("after 10 consecutive failed resumes"),
        "{paused}"
    );
    for _ in 0..4 {
        let error = f
            .prepare()
            .err()
            .expect("automatic launch is paused, not fresh");
        assert!(is_resume_blocked(&error));
    }
    assert_eq!(f.receipt(), failed, "paused reads must not start attempts");

    // An explicit Start/Restart gets exactly one more attempt per pause.
    assert!(retry_failed_resume(&f.home, &f.group, "worker").expect("explicit retry"));
    let retry = f.prepare().expect("retry").expect("same saved identity");
    assert_eq!(retry.session_id, SESSION);
    assert!(
        f.failure(&retry, false).is_some(),
        "still failing: pause again"
    );
    assert!(is_resume_blocked(&f.prepare().err().expect("paused again")));

    // A successful resume resets the count.
    assert!(retry_failed_resume(&f.home, &f.group, "worker").expect("explicit retry"));
    assert!(f.prepare().expect("retry").is_some());
    f.success();
    assert_eq!(f.receipt()["failure_count"], 0);
    let attempt = f.prepare().expect("prepare").expect("saved session");
    assert_eq!(f.failure(&attempt, false), None, "a fresh count retries");
}

#[test]
fn a_blocking_failure_at_the_limit_keeps_its_own_diagnosis() {
    let f = Fixture::new();
    for _ in 1..AUTOMATIC_RESUME_FAILURE_LIMIT {
        let attempt = f.prepare().expect("prepare").expect("saved session");
        f.failure(&attempt, false);
    }
    let attempt = f.prepare().expect("prepare").expect("saved session");
    assert_eq!(
        f.failure(&attempt, true),
        None,
        "already blocked on its own"
    );
    assert_eq!(f.receipt()["status"], "resume_failed");
    assert_eq!(f.receipt()["resume_eligible"], false);
    assert_eq!(f.receipt()["last_resume_error"], "Safe fixture diagnosis");
}

#[test]
fn old_resume_failure_cannot_overwrite_a_new_attempt_or_a_success_for_the_same_id() {
    let f = Fixture::new();
    let old = f.prepare().expect("first").expect("session");
    let new = f.prepare().expect("second").expect("session");
    let pending = f.receipt();
    f.failure(&old, true);
    assert_eq!(f.receipt(), pending);
    f.failure(&new, false);
    f.failure(&new, true);
    assert_eq!(
        f.receipt()["failure_count"],
        1,
        "each attempt records one outcome"
    );
    f.success();
    let success = f.receipt();
    f.failure(&new, true);
    assert_eq!(f.receipt(), success);
}

#[test]
fn changing_launch_identity_does_not_reuse_a_blocked_conversation() {
    let mut f = Fixture::new();
    let attempt = f.prepare().expect("prepare").expect("session");
    f.failure(&attempt, true);
    f.command.extend(["--model".into(), "sonnet".into()]);
    assert!(f.prepare().expect("new configured identity").is_none());
    assert_eq!(
        f.receipt()["provider_session_id"],
        SESSION,
        "preparation must not erase the old binding"
    );
}
