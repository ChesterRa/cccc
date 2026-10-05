//! Cursor's blocking user interactions are distinct from tool permission.
use serde_json::Value;
use std::{collections::HashSet, io};

pub(super) fn is_cursor_interaction(message: &Value, runtime: &str) -> bool {
    runtime == "cursor"
        && message.get("id").is_some()
        && matches!(
            message["method"].as_str(),
            Some("cursor/ask_question" | "cursor/create_plan")
        )
}

pub(super) fn valid_request(message: &Value) -> bool {
    let params = &message["params"];
    if !params["toolCallId"].is_string() {
        return false;
    }
    match message["method"].as_str() {
        Some("cursor/create_plan") => params["plan"].is_string(),
        Some("cursor/ask_question") => params["questions"].as_array().is_some_and(|questions| {
            let mut ids = HashSet::new();
            !questions.is_empty()
                && questions.iter().all(|question| {
                    question["id"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty() && ids.insert(id))
                        && question["prompt"].is_string()
                        && question["options"].as_array().is_some_and(|options| {
                            let mut ids = HashSet::new();
                            !options.is_empty()
                                && options.iter().all(|option| {
                                    option["id"]
                                        .as_str()
                                        .is_some_and(|id| !id.is_empty() && ids.insert(id))
                                        && option["label"].is_string()
                                })
                        })
                })
        }),
        _ => false,
    }
}

pub(super) fn response(request: &Value, reply: &Value) -> io::Result<Value> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Answer does not match the pending ACP interaction",
        )
    };
    if serde_json::to_vec(reply).map_err(io::Error::other)?.len() > 64 * 1024 {
        return Err(invalid());
    }
    let outcome = reply["outcome"]["outcome"].as_str().ok_or_else(invalid)?;
    if let Some(reason) = reply["outcome"].get("reason")
        && !reason.is_string()
    {
        return Err(invalid());
    }
    match (request["method"].as_str(), outcome) {
        (Some("cursor/ask_question" | "cursor/create_plan"), "cancelled")
        | (Some("cursor/ask_question"), "skipped")
        | (Some("cursor/create_plan"), "accepted" | "rejected") => {}
        (Some("cursor/ask_question"), "answered") => {
            let questions = request["params"]["questions"]
                .as_array()
                .ok_or_else(invalid)?;
            let answers = reply["outcome"]["answers"].as_array().ok_or_else(invalid)?;
            if questions.is_empty() || answers.len() != questions.len() {
                return Err(invalid());
            }
            let mut seen = HashSet::new();
            for answer in answers {
                let id = answer["questionId"]
                    .as_str()
                    .filter(|id| seen.insert(*id))
                    .ok_or_else(invalid)?;
                let question = questions
                    .iter()
                    .find(|question| question["id"].as_str() == Some(id))
                    .ok_or_else(invalid)?;
                let selections = answer["selectedOptionIds"].as_array().ok_or_else(invalid)?;
                if selections.is_empty()
                    || (question["allowMultiple"] != true && selections.len() != 1)
                {
                    return Err(invalid());
                }
                let options = question["options"].as_array().ok_or_else(invalid)?;
                let mut selected = HashSet::new();
                for option in selections {
                    let id = option
                        .as_str()
                        .filter(|id| selected.insert(*id))
                        .ok_or_else(invalid)?;
                    if !options
                        .iter()
                        .any(|option| option["id"].as_str() == Some(id))
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        _ => return Err(invalid()),
    }
    // Return only fields supported by the provider, never arbitrary client RPC.
    let mut result = serde_json::json!({"outcome":{"outcome":outcome}});
    if outcome == "answered" {
        result["outcome"]["answers"] = reply["outcome"]["answers"].clone();
    }
    if matches!(outcome, "skipped" | "rejected")
        && let Some(reason) = reply["outcome"].get("reason")
    {
        result["outcome"]["reason"] = reason.clone();
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn question_requires_complete_valid_explicit_choices() {
        let request = json!({"method":"cursor/ask_question","params":{"questions":[{"id":"q","options":[{"id":"a"},{"id":"b"}]}]}});
        for reply in [
            json!({"allow":true}),
            json!({"outcome":{"outcome":"answered","answers":[]}}),
            json!({"outcome":{"outcome":"answered","answers":[{"questionId":"q","selectedOptionIds":["a","b"]}]}}),
            json!({"outcome":{"outcome":"answered","answers":[{"questionId":"q","selectedOptionIds":["foreign"]}]}}),
        ] {
            assert!(response(&request, &reply).is_err());
        }
        assert!(response(&request,&json!({"outcome":{"outcome":"answered","answers":[{"questionId":"q","selectedOptionIds":["b"]}]}})).is_ok());
        assert!(response(&request, &json!({"outcome":{"outcome":"skipped"}})).is_ok());
    }
    #[test]
    fn plan_is_not_tool_approval() {
        let request = json!({"method":"cursor/create_plan"});
        assert!(response(&request, &json!({"allow":true})).is_err());
        for outcome in ["accepted", "rejected", "cancelled"] {
            assert!(response(&request, &json!({"outcome":{"outcome":outcome}})).is_ok());
        }
    }
}
