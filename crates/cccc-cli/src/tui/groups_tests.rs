use super::*;
use serde_json::json;

fn row(group_id: &str, title: &str, state: &str, running: bool, count: u64) -> Value {
    json!({
        "group_id": group_id,
        "title": title,
        "state": state,
        "running": running,
        "desired_running": running,
        "runtime_status": {"running_actor_count": count, "runtime_running": running},
    })
}

#[test]
fn group_states_read_the_way_the_web_shows_them() {
    let cases = [
        (row("g1", "a", "paused", true, 2), GroupStatus::Paused),
        (row("g2", "b", "paused", false, 0), GroupStatus::Paused),
        (row("g3", "c", "active", false, 0), GroupStatus::Offline),
        (row("g4", "d", "idle", true, 1), GroupStatus::Idle),
        (row("g5", "e", "active", true, 3), GroupStatus::Running),
        (row("g6", "f", "stopped", false, 0), GroupStatus::Stopped),
        (row("g8", "h", "stopped", true, 1), GroupStatus::Stopped),
        (row("g9", "i", "idle", false, 0), GroupStatus::Offline),
    ];
    for (value, status) in cases {
        let group = summary(&value).expect("summary");
        assert_eq!(group.status, status, "{value}");
    }
    let started_empty = summary(&row("g7", "empty", "active", false, 0)).expect("summary");
    // No running actors does not change the recorded lifecycle.
    assert_eq!(started_empty.status, GroupStatus::Offline);
    assert_eq!(started_empty.state, "active");
}

#[test]
fn rows_without_an_id_are_left_out_and_missing_fields_have_defaults() {
    assert!(summary(&json!({"title": "no id"})).is_none());
    let bare = summary(&json!({"group_id": "g_bare"})).expect("summary");
    assert_eq!(bare.name(), "g_bare");
    assert_eq!(bare.state, "active");
    assert_eq!(bare.status, GroupStatus::Offline);
    assert_eq!(bare.running_actors, 0);
}

#[test]
fn running_groups_come_first_then_by_name() {
    let mut list = GroupList::default();
    let now = Instant::now();
    let read = [
        row("g_z", "zeta", "stopped", false, 0),
        row("g_b", "Beta", "active", true, 1),
        row("g_p", "paused", "paused", false, 0),
        row("g_a", "alpha", "active", true, 2),
        row("g_i", "idle", "idle", true, 1),
    ]
    .iter()
    .filter_map(summary)
    .collect();
    list.record(Ok(read), now);
    let order: Vec<&str> = list.groups.iter().map(|g| g.group_id.as_str()).collect();
    assert_eq!(order, ["g_a", "g_b", "g_i", "g_p", "g_z"]);
    assert_eq!(list.running(), 3);
    assert_eq!(list.position("g_p"), Some(3));
    assert!(list.find("g_gone").is_none());
}

#[test]
fn a_failed_read_keeps_the_last_groups() {
    let mut list = GroupList::default();
    let first = Instant::now();
    list.record(
        Ok(vec![
            summary(&row("g1", "a", "active", true, 1)).expect("summary"),
        ]),
        first,
    );
    list.record(Err("daemon unavailable".into()), Instant::now());
    assert_eq!(list.groups.len(), 1);
    assert_eq!(list.last_ok, Some(first));
    assert_eq!(list.error.as_deref(), Some("daemon unavailable"));
}

#[test]
fn members_are_named_by_state_and_kind() {
    let actor = |id: &str, runtime: &str, enabled: bool, running: bool, external: &str| {
        json!({
            "id": id, "title": "", "runtime": runtime, "role": "peer",
            "enabled": enabled, "running": running, "external_session_id": external,
        })
    };
    let cases = [
        (
            actor("m1", "codex", true, true, ""),
            MemberState::Running,
            false,
        ),
        (
            actor("m2", "codex", true, false, ""),
            MemberState::Stopped,
            false,
        ),
        (
            actor("m3", "codex", false, false, ""),
            MemberState::Off,
            false,
        ),
        (
            actor("n1", "claude", true, true, "claude:s"),
            MemberState::Ready,
            true,
        ),
        (
            actor("n2", "pi", true, false, "pi:s"),
            MemberState::NotReceiving,
            true,
        ),
        (
            actor("n3", "opencode", false, true, "opencode:s"),
            MemberState::AccessPaused,
            true,
        ),
    ];
    for (value, state, native) in cases {
        let member = member(&value).expect("member");
        assert_eq!((member.state, member.native), (state, native), "{value}");
    }
    assert_eq!(runtime_label("claude"), "Claude Code");
    assert_eq!(runtime_label("opencode"), "OpenCode");
    assert_eq!(runtime_label("grok_web_model"), "Grok Web");
    assert_eq!(runtime_label("something-new"), "something-new");
}

#[test]
fn member_reads_for_another_group_replace_instead_of_mixing() {
    let now = Instant::now();
    let one = |id: &str| Member {
        id: id.into(),
        runtime: "Codex".into(),
        role: "peer".into(),
        native: false,
        state: MemberState::Running,
    };
    let mut list = MemberList::default();
    assert!(list.may_have_native(), "unknown members may be native");
    list.record("g1", Ok(vec![one("a")]), now);
    assert!(!list.may_have_native());
    list.record("g1", Err("slow".into()), now);
    assert_eq!(
        list.members.len(),
        1,
        "a failed read keeps the last members"
    );
    assert!(list.may_have_native(), "the members may have changed since");
    list.record("g2", Err("slow".into()), now);
    assert_eq!(list.group_id, "g2");
    assert!(list.members.is_empty(), "g1's members never show as g2's");
    assert_eq!(list.last_ok, None);
}
