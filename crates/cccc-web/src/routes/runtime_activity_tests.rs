use super::{project_snapshot, stuck_events};
use cccc_core::runtime_activity::RuntimeActivityEvent;
use chrono::{TimeZone, Utc};

fn event(id: &str, status: &str, ts: &str) -> RuntimeActivityEvent {
    RuntimeActivityEvent {
        v: 1,
        id: id.into(),
        ts: ts.into(),
        group_id: "g1".into(),
        actor_id: "peer".into(),
        runtime: "codex".into(),
        activity_id: "tool:1".into(),
        kind: "tool".into(),
        status: status.into(),
        event_type: "PreToolUse".into(),
        session_id: "session".into(),
        turn_id: Some("turn".into()),
        operation_id: Some("op".into()),
        tool_name: Some("Bash".into()),
        duration_ms: None,
    }
}

#[test]
fn snapshot_keeps_only_latest_activity_revision() {
    let now = Utc.with_ymd_and_hms(2026, 7, 28, 0, 0, 10).unwrap();
    let events = vec![
        event("started", "started", "2026-07-28T00:00:00Z"),
        event("completed", "completed", "2026-07-28T00:00:05Z"),
    ];
    let projected = project_snapshot(events, now);
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].id, "completed");
}

#[test]
fn active_tool_becomes_one_deterministic_stuck_event() {
    let now = Utc.with_ymd_and_hms(2026, 7, 28, 0, 1, 1).unwrap();
    let events = vec![event("started", "started", "2026-07-28T00:00:00Z")];
    let projected = stuck_events(&events, now);
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].id, "stuck:started");
    assert_eq!(projected[0].status, "stuck");
    assert_eq!(projected[0].duration_ms, Some(61_000));
}

#[test]
fn completed_activity_never_becomes_stuck() {
    let now = Utc.with_ymd_and_hms(2026, 7, 28, 0, 2, 0).unwrap();
    let events = vec![event("completed", "completed", "2026-07-28T00:00:00Z")];
    assert!(stuck_events(&events, now).is_empty());
}
