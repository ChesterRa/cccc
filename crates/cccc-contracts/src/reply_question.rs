//! A structured choice question on a `request_reply` message to the user, and
//! the user's answer to it.
//!
//! The structure is authoritative. The daemon generates the message `text`
//! from it, so a client that reads only text still shows the whole question.
//! A reader accepts a stored question only while that generated text still
//! matches: an appended, imported or edited record can carry a structure and
//! a text that disagree, and the user must never answer a different question
//! than the one the requester reads.
//!
//! Limits count Unicode scalar values, never bytes or UTF-16 units. Values
//! over a limit are rejected, never truncated.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: u64 = 1;
pub const MAX_CONTEXT_CHARS: usize = 4000;
pub const MAX_PROMPT_CHARS: usize = 500;
pub const MAX_LABEL_CHARS: usize = 80;
/// An option's consequence and the recommendation's reason.
pub const MAX_NOTE_CHARS: usize = 240;
pub const MAX_ANSWER_CHARS: usize = 4000;
/// Option ids in display order. A choice offers two to four of them; the
/// interface always adds its own free-text answer.
pub const OPTION_IDS: [&str; 4] = ["A", "B", "C", "D"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub v: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<QuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended: Option<Recommendation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub consequence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendation {
    pub option_id: String,
    pub reason: String,
}

/// The sender's shape: the daemon assigns option ids and takes the context
/// from the message text, so an agent never passes the context twice.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionInput {
    v: u64,
    prompt: String,
    #[serde(default)]
    options: Vec<OptionInput>,
    #[serde(default)]
    recommended: Option<Recommendation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionInput {
    label: String,
    #[serde(default)]
    consequence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    /// Kept exactly as written; emptiness is judged after trimming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
    pub limit: Option<usize>,
    pub actual: Option<usize>,
}

impl FieldError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            limit: None,
            actual: None,
        }
    }
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// What a reader may rely on for a stored message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    /// No question: an ordinary message.
    None,
    Valid(Question),
    /// A question that is malformed, of an unknown version, or no longer
    /// matches the message text. Show the text; accept only plain replies.
    Unavailable,
}

impl Question {
    /// Build the stored question from a send request's `question` argument
    /// and the message text, which becomes the question's context.
    pub fn from_request(input: &Value, text: &str) -> Result<Self, FieldError> {
        let version = input.get("v").and_then(Value::as_u64);
        if version != Some(VERSION) {
            return Err(FieldError::new(
                "question.v",
                format!("question version must be {VERSION}"),
            ));
        }
        let input: QuestionInput = serde_json::from_value(input.clone())
            .map_err(|error| FieldError::new("question", error.to_string()))?;
        let question = Self {
            v: input.v,
            context: text.trim().to_owned(),
            prompt: input.prompt.trim().to_owned(),
            options: input
                .options
                .into_iter()
                .enumerate()
                // Options past the last id stay unlabelled; `validate` refuses them.
                .map(|(index, option)| QuestionOption {
                    id: OPTION_IDS.get(index).copied().unwrap_or_default().into(),
                    label: option.label.trim().to_owned(),
                    consequence: option.consequence.trim().to_owned(),
                })
                .collect(),
            recommended: input.recommended.map(|recommended| Recommendation {
                option_id: recommended.option_id.trim().to_owned(),
                reason: recommended.reason.trim().to_owned(),
            }),
        };
        question.validate()?;
        Ok(question)
    }

    pub fn validate(&self) -> Result<(), FieldError> {
        if self.v != VERSION {
            return Err(FieldError::new(
                "question.v",
                format!("question version must be {VERSION}"),
            ));
        }
        within("question.context", &self.context, MAX_CONTEXT_CHARS)?;
        required("question.prompt", &self.prompt, MAX_PROMPT_CHARS)?;
        if self.options.len() == 1 {
            return Err(FieldError {
                limit: Some(2),
                actual: Some(1),
                ..FieldError::new(
                    "question.options",
                    "a choice needs two to four options; ask without options for a free answer",
                )
            });
        }
        if self.options.len() > OPTION_IDS.len() {
            return Err(FieldError {
                limit: Some(OPTION_IDS.len()),
                actual: Some(self.options.len()),
                ..FieldError::new("question.options", "offer at most four options")
            });
        }
        for (index, (option, id)) in self.options.iter().zip(OPTION_IDS).enumerate() {
            let field = format!("question.options[{index}]");
            if option.id != id {
                return Err(FieldError::new(
                    format!("{field}.id"),
                    format!("option ids are A, B, C and D in order; expected {id}"),
                ));
            }
            required(&format!("{field}.label"), &option.label, MAX_LABEL_CHARS)?;
            single_line(&format!("{field}.label"), &option.label)?;
            within(
                &format!("{field}.consequence"),
                &option.consequence,
                MAX_NOTE_CHARS,
            )?;
            single_line(&format!("{field}.consequence"), &option.consequence)?;
            if self.options[..index]
                .iter()
                .any(|earlier| earlier.label == option.label)
            {
                return Err(FieldError::new(
                    format!("{field}.label"),
                    "options must have different labels",
                ));
            }
        }
        if let Some(recommended) = &self.recommended {
            if self.option(&recommended.option_id).is_none() {
                return Err(FieldError::new(
                    "question.recommended.option_id",
                    "recommend one of the offered options",
                ));
            }
            required(
                "question.recommended.reason",
                &recommended.reason,
                MAX_NOTE_CHARS,
            )?;
            single_line("question.recommended.reason", &recommended.reason)?;
        }
        Ok(())
    }

    pub fn option(&self, id: &str) -> Option<&QuestionOption> {
        self.options.iter().find(|option| option.id == id)
    }

    /// The complete message text for clients that read only text.
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        if !self.context.is_empty() {
            parts.push(self.context.clone());
        }
        parts.push(self.prompt.clone());
        if !self.options.is_empty() {
            let mut lines = self
                .options
                .iter()
                .map(|option| {
                    if option.consequence.is_empty() {
                        format!("{}. {}", option.id, option.label)
                    } else {
                        format!("{}. {} — {}", option.id, option.label, option.consequence)
                    }
                })
                .collect::<Vec<_>>();
            if let Some(recommended) = &self.recommended {
                lines.push(format!(
                    "Recommended: {} — {}",
                    recommended.option_id, recommended.reason
                ));
            }
            parts.push(lines.join("\n"));
            let ids = self
                .options
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>();
            parts.push(format!(
                "Reply with {}, or your own answer.",
                ids.join(", ")
            ));
        }
        parts.join("\n\n")
    }
}

/// Read the question of a stored `chat.message`, checking it against the
/// stored text rather than trusting either one alone.
pub fn stored(data: &Map<String, Value>) -> Stored {
    let Some(value) = data.get("question").filter(|value| !value.is_null()) else {
        return Stored::None;
    };
    let Ok(question) = serde_json::from_value::<Question>(value.clone()) else {
        return Stored::Unavailable;
    };
    let text = data.get("text").and_then(Value::as_str).unwrap_or_default();
    if question.validate().is_err() || question.text() != text {
        return Stored::Unavailable;
    }
    Stored::Valid(question)
}

impl Answer {
    pub fn parse(value: &Value) -> Result<Self, FieldError> {
        let answer: Self = serde_json::from_value(value.clone())
            .map_err(|error| FieldError::new("answer", error.to_string()))?;
        if answer.v != VERSION {
            return Err(FieldError::new(
                "answer.v",
                format!("answer version must be {VERSION}"),
            ));
        }
        match (&answer.option_id, &answer.text) {
            (Some(option_id), None) if !option_id.trim().is_empty() => {}
            (None, Some(text)) => {
                if text.trim().is_empty() {
                    return Err(FieldError::new("answer.text", "write an answer"));
                }
                within("answer.text", text, MAX_ANSWER_CHARS)?;
            }
            _ => {
                return Err(FieldError::new(
                    "answer",
                    "give either option_id or text, not both",
                ));
            }
        }
        Ok(answer)
    }

    /// The reply text a requester reads. A choice names the option; free text
    /// stays exactly as written.
    pub fn reply_text(&self, question: &Question) -> Result<String, FieldError> {
        if let Some(option_id) = &self.option_id {
            let option = question.option(option_id).ok_or_else(|| {
                FieldError::new("answer.option_id", "choose one of the offered options")
            })?;
            return Ok(format!("Selected {}: {}", option.id, option.label));
        }
        let text = self.text.clone().unwrap_or_default();
        Ok(if question.options.is_empty() {
            text
        } else {
            format!("Other: {text}")
        })
    }
}

fn required(field: &str, value: &str, limit: usize) -> Result<(), FieldError> {
    if value.trim().is_empty() {
        return Err(FieldError::new(field, "must not be empty"));
    }
    within(field, value, limit)
}

fn within(field: &str, value: &str, limit: usize) -> Result<(), FieldError> {
    let actual = value.chars().count();
    if actual > limit {
        return Err(FieldError {
            limit: Some(limit),
            actual: Some(actual),
            ..FieldError::new(field, format!("must be at most {limit} characters"))
        });
    }
    Ok(())
}

fn single_line(field: &str, value: &str) -> Result<(), FieldError> {
    if value.contains(['\n', '\r']) {
        return Err(FieldError::new(field, "must be a single line"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn choice() -> Value {
        json!({
            "v": 1,
            "prompt": "Where should the task count go?",
            "options": [
                {"label": "Group page only", "consequence": "the main screen stays simple"},
                {"label": "Add a column"}
            ],
            "recommended": {"option_id": "A", "reason": "60 columns is a hard limit"}
        })
    }

    #[test]
    fn a_request_becomes_a_question_with_ordered_ids_and_its_full_text() {
        let question =
            Question::from_request(&choice(), "  The TUI is narrow.\n").expect("question");
        assert_eq!(question.context, "The TUI is narrow.");
        assert_eq!(
            question
                .options
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(
            question.text(),
            "The TUI is narrow.\n\nWhere should the task count go?\n\n\
             A. Group page only — the main screen stays simple\n\
             B. Add a column\n\
             Recommended: A — 60 columns is a hard limit\n\n\
             Reply with A, B, or your own answer."
        );
    }

    #[test]
    fn a_choice_offers_up_to_four_options() {
        let question = Question::from_request(
            &json!({
                "v": 1,
                "prompt": "Which?",
                "options": [{"label": "one"}, {"label": "two"}, {"label": "three"}, {"label": "four"}],
                "recommended": {"option_id": "D", "reason": "last"}
            }),
            "",
        )
        .expect("question");
        assert_eq!(
            question.text(),
            "Which?\n\nA. one\nB. two\nC. three\nD. four\nRecommended: D — last\n\n\
             Reply with A, B, C, D, or your own answer."
        );
        let selected = Answer::parse(&json!({"v": 1, "option_id": "D"})).expect("choice");
        assert_eq!(
            selected.reply_text(&question).expect("text"),
            "Selected D: four"
        );
    }

    #[test]
    fn a_question_without_options_asks_for_free_input_only() {
        let question = Question::from_request(
            &json!({"v": 1, "prompt": "Which directory should I use?"}),
            "",
        )
        .expect("question");
        assert_eq!(question.text(), "Which directory should I use?");
    }

    #[test]
    fn invalid_requests_name_the_field_and_the_limit() {
        let cases = [
            (json!({"v": 2, "prompt": "p"}), "question.v"),
            (json!({"v": 1, "prompt": " "}), "question.prompt"),
            (
                json!({"v": 1, "prompt": "p", "options": [{"label": "only"}]}),
                "question.options",
            ),
            (
                json!({"v": 1, "prompt": "p", "options": [{"label": "a"}, {"label": "b"}, {"label": "c"}, {"label": "d"}, {"label": "e"}]}),
                "question.options",
            ),
            (
                json!({"v": 1, "prompt": "p", "options": [{"label": "same"}, {"label": "same"}]}),
                "question.options[1].label",
            ),
            (
                json!({"v": 1, "prompt": "p", "options": [{"label": "a\nb"}, {"label": "c"}]}),
                "question.options[0].label",
            ),
            (
                json!({"v": 1, "prompt": "p", "options": [{"label": "a"}, {"label": "b"}], "recommended": {"option_id": "C", "reason": "r"}}),
                "question.recommended.option_id",
            ),
            (
                json!({"v": 1, "prompt": "p", "recommended": {"option_id": "A", "reason": "r"}}),
                "question.recommended.option_id",
            ),
            (json!({"v": 1, "prompt": "p", "extra": true}), "question"),
        ];
        for (input, field) in cases {
            let error = Question::from_request(&input, "").expect_err("invalid");
            assert_eq!(error.field, field, "{input}");
        }
        let long = Question::from_request(&json!({"v": 1, "prompt": "é".repeat(501)}), "")
            .expect_err("too long");
        assert_eq!(
            (error_fields(&long)),
            ("question.prompt", Some(500), Some(501))
        );
        // Limits count characters, not bytes: 500 two-byte characters fit.
        Question::from_request(&json!({"v": 1, "prompt": "é".repeat(500)}), "").expect("fits");
    }

    fn error_fields(error: &FieldError) -> (&str, Option<usize>, Option<usize>) {
        (error.field.as_str(), error.limit, error.actual)
    }

    #[test]
    fn a_stored_question_is_trusted_only_while_its_text_matches() {
        let question = Question::from_request(&choice(), "context").expect("question");
        let data = |text: &str, value: Value| {
            json!({"text": text, "question": value})
                .as_object()
                .cloned()
                .expect("data")
        };
        let value = serde_json::to_value(&question).expect("value");
        assert_eq!(
            stored(&data(&question.text(), value.clone())),
            Stored::Valid(question.clone())
        );
        assert_eq!(
            stored(&data("a different text", value.clone())),
            Stored::Unavailable
        );
        let mut reordered = value.clone();
        reordered["options"][0]["id"] = json!("B");
        reordered["options"][1]["id"] = json!("A");
        assert_eq!(
            stored(&data(&question.text(), reordered)),
            Stored::Unavailable
        );
        let mut future = value;
        future["v"] = json!(2);
        assert_eq!(stored(&data(&question.text(), future)), Stored::Unavailable);
        assert_eq!(
            stored(&json!({"text": "plain"}).as_object().cloned().expect("data")),
            Stored::None
        );
    }

    #[test]
    fn answers_choose_one_offered_option_or_keep_free_text_verbatim() {
        let question = Question::from_request(&choice(), "").expect("question");
        let selected = Answer::parse(&json!({"v": 1, "option_id": "B"})).expect("choice");
        assert_eq!(
            selected.reply_text(&question).expect("text"),
            "Selected B: Add a column"
        );
        let unknown = Answer::parse(&json!({"v": 1, "option_id": "C"})).expect("shape");
        assert_eq!(
            unknown
                .reply_text(&question)
                .expect_err("not offered")
                .field,
            "answer.option_id"
        );
        let written = "  both, but the group page first\n";
        let other = Answer::parse(&json!({"v": 1, "text": written})).expect("text");
        assert_eq!(other.text.as_deref(), Some(written));
        assert_eq!(
            other.reply_text(&question).expect("text"),
            format!("Other: {written}")
        );
        let open = Question::from_request(&json!({"v": 1, "prompt": "Path?"}), "").expect("open");
        assert_eq!(other.reply_text(&open).expect("text"), written);
        for invalid in [
            json!({"v": 1}),
            json!({"v": 1, "option_id": "A", "text": "x"}),
            json!({"v": 1, "text": " \n "}),
            json!({"v": 2, "option_id": "A"}),
            json!({"v": 1, "option_id": "A", "extra": 1}),
            json!({"v": 1, "text": "x".repeat(MAX_ANSWER_CHARS + 1)}),
        ] {
            assert!(Answer::parse(&invalid).is_err(), "{invalid}");
        }
    }
}
