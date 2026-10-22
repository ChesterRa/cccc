use super::*;

fn captured(id: &str) -> Space {
    let rows: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/host-disk-samples.json"))
            .expect("captured samples");
    let row = rows
        .as_array()
        .expect("samples array")
        .iter()
        .find(|row| row["id"] == id)
        .expect("sample id");
    let block = row["f_frsize"].as_u64().expect("block size");
    let space = Space {
        available_bytes: row["f_bavail"].as_u64().expect("available blocks") * block,
        total_bytes: row["f_blocks"].as_u64().expect("total blocks") * block,
    };
    assert_eq!(space.available_bytes, row["available_bytes"]);
    assert_eq!(space.total_bytes, row["total_bytes"]);
    space
}

#[test]
fn captured_samples_enter_low_and_require_recovery() {
    let mut diagnostic = Diagnostic::default();
    assert_eq!(diagnostic.state, "unknown");
    for (sample, expected) in [
        ("volume_a", "healthy"),
        ("volume_b", "healthy"),
        ("volume_c", "low"),
        ("volume_b", "low"),
        ("volume_a", "healthy"),
    ] {
        diagnostic.record(Ok(captured(sample)));
        assert_eq!(diagnostic.state, expected);
        assert!(diagnostic.error.is_none());
    }
    assert_eq!(diagnostic.attempts, 5);
}

#[test]
fn captured_capacity_at_each_policy_boundary_is_not_low() {
    // The observations stay verbatim. Place the policy boundaries at two
    // captured values to test strict entry and inclusive recovery precisely.
    let mut diagnostic = Diagnostic {
        low_bytes: captured("volume_b").available_bytes,
        recovery_bytes: captured("volume_a").available_bytes,
        ..Default::default()
    };
    diagnostic.record(Ok(captured("volume_b")));
    assert_eq!(diagnostic.state, "healthy");
    diagnostic.record(Ok(captured("volume_c")));
    assert_eq!(diagnostic.state, "low");
    diagnostic.record(Ok(captured("volume_b")));
    assert_eq!(diagnostic.state, "low");
    diagnostic.record(Ok(captured("volume_a")));
    assert_eq!(diagnostic.state, "healthy");
}

#[test]
fn failed_sample_is_visible_and_cannot_clear_low_hysteresis() {
    let temp = tempfile::tempdir().expect("probe directory");
    let mut diagnostic = Diagnostic::default();
    diagnostic.record(Ok(captured("volume_c")));
    let old = diagnostic.clone();
    diagnostic.record(sample(&temp.path().join("missing")));
    assert_eq!(diagnostic.state, "unavailable");
    assert!(diagnostic.error.is_some());
    assert_eq!(diagnostic.sampled_at, old.sampled_at);
    assert_eq!(
        diagnostic
            .space
            .as_ref()
            .expect("last sample")
            .available_bytes,
        old.space.as_ref().expect("old sample").available_bytes
    );
    diagnostic.record(Ok(captured("volume_b")));
    assert_eq!(diagnostic.state, "low");
    assert!(diagnostic.error.is_none());
    diagnostic.record(Ok(captured("volume_a")));
    assert_eq!(diagnostic.state, "healthy");
}

#[test]
fn small_filesystem_probe_succeeds_and_reports_a_missing_path() {
    let temp = tempfile::tempdir().expect("probe directory");
    std::fs::write(temp.path().join("one-byte"), b"x").expect("small write");
    let result = sample(temp.path()).expect("real filesystem sample");
    assert!(result.total_bytes > 0);
    assert!(result.available_bytes <= result.total_bytes);
    assert!(sample(&temp.path().join("missing")).is_err());
}

async fn wait_attempts(home: &HomeLayout, attempts: u64) -> Diagnostic {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(value) = snapshot(home)
                && value.attempts >= attempts
            {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("independent sampler progressed")
}

#[tokio::test]
async fn host_timer_samples_without_groups_or_notice_ticks() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home layout");
    let service = HostDiskService::with_interval(home.clone(), Duration::from_millis(20));
    // No Group, automation scheduler, notice tick or status poll is needed.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let diagnostic = wait_attempts(&home, 2).await;
    assert!(diagnostic.space.is_some());
    assert!(diagnostic.error.is_none());
    service.finish().await;
    assert!(snapshot(&home).is_none(), "shutdown releases cached state");
}

#[tokio::test]
async fn status_reads_are_cached_and_home_scoped() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home layout");
    let other = HomeLayout::from_path(temp.path().join("other")).expect("other home");
    let service = HostDiskService::start(home.clone());
    let initial = wait_attempts(&home, 1).await;
    for _ in 0..100 {
        let current = snapshot(&home).expect("cached status");
        assert_eq!(current.attempts, initial.attempts);
        assert_eq!(current.sampled_at, initial.sampled_at);
        assert!(snapshot(&other).is_none());
    }
    service.finish().await;
    assert!(snapshot(&home).is_none());
}
