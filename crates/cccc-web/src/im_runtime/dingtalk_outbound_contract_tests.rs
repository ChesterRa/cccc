use super::*;

#[test]
fn routes_only_complete_known_conversation_types() {
    assert!(matches!(
        route_target(&DingTalkTarget {
            chat_id: "cid-private".into(),
            robot_code: String::new(),
            conversation_type: "1".into(),
            user_id: "staff-1".into(),
        }),
        Ok((OTO_ENDPOINT, "staff-1", true))
    ));
    assert!(matches!(
        route_target(&DingTalkTarget {
            chat_id: "cid-group".into(),
            robot_code: String::new(),
            conversation_type: "2".into(),
            user_id: String::new(),
        }),
        Ok((GROUP_ENDPOINT, "cid-group", false))
    ));
    for (conversation_type, chat_id, user_id) in [
        ("1", "cid-private", ""),
        ("2", "", ""),
        ("", "cid-group", "staff-1"),
        ("3", "cid-group", "staff-1"),
    ] {
        assert!(
            route_target(&DingTalkTarget {
                chat_id: chat_id.into(),
                robot_code: String::new(),
                conversation_type: conversation_type.into(),
                user_id: user_id.into(),
            })
            .is_err()
        );
    }
}

#[test]
fn openapi_response_requires_explicit_complete_success() {
    assert!(
        validate_openapi_response(StatusCode::OK, br#"{"processQueryKey":"accepted"}"#).is_ok()
    );
    assert!(
        validate_openapi_response(
            StatusCode::OK,
            br#"{"sendResults":[{"success":true},{"status":"SUCCESS"}]}"#
        )
        .is_ok()
    );
    for (status, body) in [
        (
            StatusCode::UNAUTHORIZED,
            br#"{"message":"unauthorized"}"#.as_slice(),
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            b"upstream failed".as_slice(),
        ),
        (StatusCode::OK, b"not-json".as_slice()),
        (StatusCode::OK, br#"{}"#.as_slice()),
        (StatusCode::OK, br#"{"processQueryKey":""}"#.as_slice()),
        (StatusCode::OK, br#"{"sendResults":[]}"#.as_slice()),
        (
            StatusCode::OK,
            br#"{"sendResults":[{"success":false}]}"#.as_slice(),
        ),
        (
            StatusCode::OK,
            br#"{"sendResults":[{"success":true},{"status":"FAILED"}]}"#.as_slice(),
        ),
        (
            StatusCode::OK,
            br#"{"sendResults":[{"success":true,"code":500,"status":"FAILED"}]}"#.as_slice(),
        ),
    ] {
        assert!(validate_openapi_response(status, body).is_err());
    }
}

#[test]
fn loaded_bytes_are_checked_again_after_read() {
    assert!(validate_loaded_size(&vec![0; MAX_ATTACHMENT_BYTES as usize]).is_ok());
    assert!(validate_loaded_size(&vec![0; MAX_ATTACHMENT_BYTES as usize + 1]).is_err());
}
