use super::*;

pub(super) fn assert_resize_ownership(home: &HomeLayout, group_id: &str) {
    let invalid_attachment = raw_call(
        home,
        "term_resize",
        json!({"group_id":group_id,"actor_id":"peer1","attachment_id":0,"cols":100,"rows":30}),
    );
    assert_eq!(
        invalid_attachment
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("invalid_args")
    );
    let first_controller = cccc_runtime::attach(
        group_id,
        "peer1",
        cccc_runtime::TerminalAttachMode::Control,
        false,
        None,
    )
    .expect("first terminal controller");
    let takeover_controller = cccc_runtime::attach(
        group_id,
        "peer1",
        cccc_runtime::TerminalAttachMode::Control,
        true,
        None,
    )
    .expect("takeover terminal controller");
    let stale_resize = raw_call(
        home,
        "term_resize",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "attachment_id":first_controller.attachment_id(),
            "cols":120,
            "rows":40
        }),
    );
    assert_eq!(
        stale_resize.error.as_ref().map(|error| error.code.as_str()),
        Some("terminal_not_writer")
    );
    assert!(
        call(
            home,
            "term_resize",
            json!({
                "group_id":group_id,
                "actor_id":"peer1",
                "attachment_id":takeover_controller.attachment_id(),
                "cols":120,
                "rows":40
            }),
        )
        .ok
    );
    drop(takeover_controller);
    drop(first_controller);
}
