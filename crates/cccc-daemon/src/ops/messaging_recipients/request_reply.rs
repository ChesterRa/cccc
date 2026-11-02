use cccc_core::{GroupDoc, actors};

use crate::dispatch::OpError;

pub(super) fn normalize_targets(
    group: &GroupDoc,
    message_mode: &str,
    raw: Vec<String>,
) -> Result<Vec<String>, OpError> {
    if message_mode != "request_reply" {
        return Ok(raw);
    }

    let raw = raw
        .into_iter()
        .filter(|recipient| !recipient.trim().is_empty())
        .collect::<Vec<_>>();
    if raw
        .iter()
        .any(|recipient| matches!(recipient.trim(), "@all" | "@peers"))
    {
        return Err(OpError::new(
            "concrete_recipients_required",
            "request_reply requires the default foreman or one or more concrete recipients",
        ));
    }
    if !raw.is_empty() && !raw.iter().any(|recipient| recipient.trim() == "@foreman") {
        return Ok(raw);
    }

    let foreman = actors::unique_available_foreman(group).map_err(|error| match error {
        actors::UniqueForemanError::NotFound => {
            OpError::new("foreman_not_found", "group has no available foreman")
        }
        actors::UniqueForemanError::NotUnique => OpError::new(
            "foreman_not_unique",
            "group has more than one available foreman",
        ),
    })?;
    let mut normalized = Vec::with_capacity(raw.len().max(1));
    for recipient in raw
        .iter()
        .map(String::as_str)
        .chain(raw.is_empty().then_some("@foreman"))
    {
        let recipient = if recipient.trim() == "@foreman" {
            foreman.id.as_str()
        } else {
            recipient
        };
        if !normalized.iter().any(|item| item == recipient) {
            normalized.push(recipient.to_owned());
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use cccc_contracts::{Actor, GroupState};
    use cccc_core::GroupDoc;
    use serde_json::{Map, Value, json};

    fn group() -> GroupDoc {
        GroupDoc {
            generation: String::new(),
            v: 1,
            group_id: "g_test".into(),
            title: "test".into(),
            topic: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            running: false,
            state: GroupState::Active,
            active_scope_key: String::new(),
            scopes: Vec::new(),
            actors: vec![Actor::new("lead"), Actor::new("peer")],
            automation: Map::new(),
            extra: Map::new(),
        }
    }

    #[test]
    fn default_and_foreman_alias_resolve_to_the_concrete_foreman() {
        for recipients in [json!([]), json!(["@foreman"])] {
            let mut data = json!({
                "text": "please reply",
                "to": recipients,
                "message_mode": "request_reply"
            })
            .as_object()
            .expect("message")
            .clone();

            super::super::normalize_chat_preflight(&group(), "user", &mut data, false)
                .expect("normalize request_reply");

            assert_eq!(data["to"], json!(["lead"]));
        }
    }

    #[test]
    fn broadcast_request_reply_remains_rejected() {
        let mut data = json!({
            "text": "please reply",
            "to": ["@all"],
            "message_mode": "request_reply"
        })
        .as_object()
        .expect("message")
        .clone();

        let error = super::super::normalize_chat_preflight(&group(), "user", &mut data, false)
            .expect_err("broadcast must fail");

        assert_eq!(error.code, "concrete_recipients_required");
    }

    #[test]
    fn chet_is_its_own_audience_and_does_not_accept_mail() {
        let normalize = |to: Value, mode: &str, by: &str| {
            let mut data = json!({"text":"hello","to":to,"message_mode":mode})
                .as_object()
                .expect("message")
                .clone();
            super::super::normalize_chat_preflight(&group(), by, &mut data, false).map(|()| data)
        };
        for mode in ["send", "request_reply"] {
            let data = normalize(json!(["@chet"]), mode, "peer").expect("Chet audience");
            assert_eq!(
                data["to"],
                json!([cccc_core::actors::CHET_SERVICE_PRINCIPAL])
            );
        }
        assert!(normalize(json!(["@chet"]), "mail", "peer").is_err());
        assert!(normalize(json!(["@chet", "user"]), "send", "peer").is_err());
        assert!(normalize(json!(["@chet", "lead"]), "send", "peer").is_err());
        assert!(
            normalize(
                json!(["@chet"]),
                "send",
                cccc_core::actors::CHET_SERVICE_PRINCIPAL
            )
            .is_err()
        );
        assert_eq!(
            normalize(json!(["@chet", "@CHET"]), "send", "peer")
                .expect("duplicate service selector")["to"],
            json!([cccc_core::actors::CHET_SERVICE_PRINCIPAL])
        );
    }

    #[test]
    fn im_cannot_address_chet_via_group_send() {
        let mut data =
            json!({"text":"hello","to":["@chet"],"message_mode":"send","transport":"im"})
                .as_object()
                .expect("message")
                .clone();
        let error = super::super::normalize_chat_preflight(&group(), "user", &mut data, false)
            .expect_err("IM must not access Chet through user");
        assert_eq!(error.code, "chet_im_unsupported");
    }

    #[test]
    fn cross_group_routes_do_not_address_the_local_chet_service() {
        for sender in ["g_other::peer", "connect:source"] {
            let mut data = json!({"text":"hello","to":["@chet"],"message_mode":"send"})
                .as_object()
                .expect("message")
                .clone();
            let error = super::super::normalize_chat_preflight(&group(), sender, &mut data, false)
                .expect_err("cross-group Chet address must fail");
            assert_eq!(error.code, "chet_cross_group_unsupported");
        }
    }

    #[test]
    fn existing_member_title_matching_service_identity_requires_its_actor_id() {
        let mut group = group();
        group.actors[0].title = "svc:chet".into();
        let mut data = json!({"text":"hello","to":["svc:chet"],"message_mode":"send"})
            .as_object()
            .expect("message")
            .clone();
        assert!(super::super::normalize_chat_preflight(&group, "peer", &mut data, false).is_err());
        data.insert("to".into(), json!(["lead"]));
        super::super::normalize_chat_preflight(&group, "peer", &mut data, false).expect("actor ID");
        assert_eq!(data["to"], json!(["lead"]));
    }
}
