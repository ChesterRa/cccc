use serde_json::{Map, Value};

use crate::api::ApiError;

/// Ordinary Web messaging always represents the signed-in human user. Runtime
/// bindings and Chet's origin are supplied by their own host-side ports.
pub(in crate::routes) fn prepare_user_message(
    args: &mut Map<String, Value>,
    is_admin: bool,
) -> Result<(), ApiError> {
    if let Some(by) = args.get("by")
        && !by.is_null()
        && !by
            .as_str()
            .is_some_and(|value| value.trim().is_empty() || value == "user")
    {
        return Err(ApiError::forbidden("Web messages must be sent as user"));
    }
    if args
        .keys()
        .any(|key| key == "_voice_origin" || key.starts_with("_cccc_"))
    {
        return Err(ApiError::forbidden(
            "runtime authority cannot be supplied by a Web message",
        ));
    }
    args.insert("by".into(), Value::String("user".into()));
    args.insert(
        cccc_core::actors::CHET_WEB_ADMISSION_ARG.into(),
        Value::Bool(is_admin),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::prepare_user_message;

    #[test]
    fn ordinary_web_messages_use_the_user_identity() {
        for by in [
            None,
            Some(json!(null)),
            Some(json!("")),
            Some(json!("user")),
        ] {
            let mut args = serde_json::Map::new();
            if let Some(value) = by {
                args.insert("by".into(), value);
            }
            prepare_user_message(&mut args, false).expect("human message");
            assert_eq!(args["by"], "user");
            assert_eq!(args[cccc_core::actors::CHET_WEB_ADMISSION_ARG], false);
        }
    }

    #[test]
    fn ordinary_web_messages_reject_runtime_authority() {
        for value in [
            json!({"by":"worker"}),
            json!({"by":"svc:chet"}),
            json!({"_voice_origin":"invalid"}),
            json!({"_cccc_web_binding":{}}),
        ] {
            let mut args = value.as_object().expect("object").clone();
            assert!(prepare_user_message(&mut args, true).is_err());
        }
    }
}
