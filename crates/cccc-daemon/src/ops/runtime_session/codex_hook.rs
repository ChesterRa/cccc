use cccc_core::HomeLayout;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Observation {
    Ready(String),
    Pending,
    Unavailable,
}

pub(super) fn observe(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    pid: Option<u32>,
) -> Observation {
    let Some(capability) =
        crate::ops::runtime_hook_session::validated(home, "codex", group_id, actor_id, pid)
    else {
        return Observation::Unavailable;
    };
    let Some(state) = cccc_core::codex_hook_state::read(home, group_id, actor_id) else {
        return Observation::Pending;
    };
    if state.launch_token != capability.launch_token
        || state.awaiting_session_start
        || state.session_closed
    {
        return Observation::Pending;
    }

    let session_id = state.session_id.trim();
    if super::valid_session_id(session_id) {
        Observation::Ready(session_id.to_owned())
    } else {
        Observation::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ACTOR_ID: &str = "peer1";
    const PID: u32 = 4242;
    const SESSION_ID: &str = "019fea2e-ea50-7b43-9fc7-efd55e70a585";

    fn launch() -> (tempfile::TempDir, HomeLayout, String, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize home");
        let group_id = format!("g_{}", uuid::Uuid::new_v4().simple());
        let token = uuid::Uuid::new_v4().simple().to_string();
        cccc_core::codex_hook_state::begin_launch(
            &home,
            "codex",
            &group_id,
            ACTOR_ID,
            &token,
            "HookPending",
        )
        .expect("begin hook launch");
        crate::ops::runtime_hook_session::bind_for_test(
            &home, &group_id, ACTOR_ID, "codex", &token, PID,
        );
        (temp, home, group_id, token)
    }

    #[test]
    fn accepts_only_the_current_fenced_session_start() {
        let (_temp, home, group_id, token) = launch();
        assert_eq!(
            observe(&home, &group_id, ACTOR_ID, Some(PID)),
            Observation::Pending
        );

        cccc_core::codex_hook_state::record(
            &home,
            &group_id,
            ACTOR_ID,
            &token,
            &json!({"hook_event_name":"SessionStart","session_id":SESSION_ID}),
        )
        .expect("record SessionStart");

        assert_eq!(
            observe(&home, &group_id, ACTOR_ID, Some(PID)),
            Observation::Ready(SESSION_ID.into())
        );
        assert_eq!(
            observe(&home, &group_id, ACTOR_ID, Some(PID + 1)),
            Observation::Unavailable
        );
        crate::ops::runtime_hook_session::revoke(&group_id, ACTOR_ID);
    }

    #[test]
    fn leaves_non_uuid_hook_sessions_pending_for_the_status_fallback() {
        let (_temp, home, group_id, token) = launch();
        cccc_core::codex_hook_state::record(
            &home,
            &group_id,
            ACTOR_ID,
            &token,
            &json!({"hook_event_name":"SessionStart","session_id":"not-a-uuid"}),
        )
        .expect("record SessionStart");

        assert_eq!(
            observe(&home, &group_id, ACTOR_ID, Some(PID)),
            Observation::Pending
        );
        crate::ops::runtime_hook_session::revoke(&group_id, ACTOR_ID);
    }
}
