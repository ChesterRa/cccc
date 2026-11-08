use serde_json::{Map, Value, json};

pub(crate) fn alias(args: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = args.remove(from) {
        args.entry(to).or_insert(value);
    }
}

/// MCP uses `mode`; `message_mode` belongs to the daemon IPC contract.
pub(crate) fn message_mode(args: &mut Map<String, Value>, default: &str) -> Result<String, String> {
    if args.contains_key("message_mode") {
        return Err("invalid_args: MCP message tools use mode, not message_mode; remove message_mode and pass mode explicitly".into());
    }
    match args.remove("mode") {
        None => Ok(default.into()),
        Some(Value::String(mode)) if matches!(mode.as_str(), "send" | "request_reply" | "mail") => {
            Ok(mode)
        }
        _ => Err("invalid_args: mode must be send, request_reply or mail".into()),
    }
}

/// The mode of a new message, or of a file sent as one, whose caller named none. The user has
/// no Inbox, so a message to the user is sent; a native session sends, as its guidance says; a
/// managed member's message to agents is stored as Mail. Every tool that starts a message takes
/// its default from here, so a new one cannot drift from the others.
pub(crate) fn default_new_message_mode(args: &Map<String, Value>, native: bool) -> &'static str {
    let to_user = match args.get("to") {
        Some(Value::String(recipient)) => is_user(recipient),
        Some(Value::Array(recipients)) => recipients.iter().filter_map(Value::as_str).any(is_user),
        _ => false,
    };
    if to_user || native { "send" } else { "mail" }
}

/// A recipient the daemon would drop, such as a number in `to`, is refused rather than ignored,
/// so the default chosen from the remaining recipients never sends what the caller did not ask.
pub(crate) fn check_recipients(args: &Map<String, Value>) -> Result<(), String> {
    let valid = match args.get("to") {
        None | Some(Value::Null | Value::String(_)) => true,
        Some(Value::Array(recipients)) => recipients.iter().all(Value::is_string),
        Some(_) => false,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid_recipient: to must be a recipient ID or a list of recipient IDs".into())
    }
}

fn is_user(recipient: &str) -> bool {
    matches!(recipient.trim(), "user" | "@user")
}

pub(crate) fn normalize_recipients(args: &mut Map<String, Value>) {
    if let Some(Value::String(value)) = args.get("to").cloned() {
        args.insert("to".into(), json!([value]));
    }
}

pub(crate) fn normalize_message_author(args: &mut Map<String, Value>) {
    if args
        .get("by")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }
    if let Some(actor_id) = args
        .get("actor_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    {
        args.insert("by".into(), Value::String(actor_id));
    }
}

#[cfg(test)]
mod tests {
    use super::{check_recipients, default_new_message_mode};
    use serde_json::json;

    #[test]
    fn a_new_message_to_the_user_is_sent_and_native_sessions_send_by_default() {
        let mode = |args: serde_json::Value, native| {
            default_new_message_mode(args.as_object().expect("args"), native)
        };
        // The user has no Inbox, whichever way the recipient is written.
        for to in [json!("user"), json!(["@user"]), json!([" user "])] {
            assert_eq!(mode(json!({"to": to}), false), "send", "{to}");
            assert_eq!(mode(json!({"to": to}), true), "send", "{to}");
        }
        // Agents: Mail for a managed member, Send in a native session.
        for to in [json!("lead"), json!(["lead", "@peers"]), json!([])] {
            assert_eq!(mode(json!({"to": to}), false), "mail", "{to}");
            assert_eq!(mode(json!({"to": to}), true), "send", "{to}");
        }
        assert_eq!(mode(json!({}), false), "mail");
        assert_eq!(mode(json!({}), true), "send");
    }

    #[test]
    fn recipients_the_daemon_would_drop_are_refused() {
        let check = |args: serde_json::Value| check_recipients(args.as_object().expect("args"));
        for to in [
            json!("user"),
            json!(["lead", "@all"]),
            json!([]),
            json!(null),
        ] {
            assert!(check(json!({"to": to})).is_ok(), "{to}");
        }
        assert!(check(json!({})).is_ok());
        for to in [
            json!(["user", 42]),
            json!([null]),
            json!(7),
            json!({"id": "user"}),
        ] {
            assert!(check(json!({"to": to})).is_err(), "{to}");
        }
    }
}
