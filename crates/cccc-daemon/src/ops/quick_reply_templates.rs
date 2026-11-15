//! Reply templates a person keeps for the Web: a short name shown in menus and the text that is
//! sent from a message's quick replies or inserted into the composer. Each access identity has
//! one list, shared by every browser and device it signs in from; agents cannot read or change it.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::PathBuf;

use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{HomeLayout, fs};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use crate::dispatch::{OpError, OpResult, object, string_arg};

const VERSION: u8 = 1;
const MAX_TEMPLATES: usize = 20;
const MAX_NAME_CHARS: usize = 40;
const MAX_TEXT_CHARS: usize = 10_000;
const MAX_ID_CHARS: usize = 64;
/// The loopback administrator, and local CLI requests that name no Web user.
const LOCAL_OWNER: &str = "local";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Document {
    v: u8,
    #[serde(default)]
    owners: BTreeMap<String, OwnerTemplates>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            v: VERSION,
            owners: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OwnerTemplates {
    revision: u64,
    updated_at: String,
    templates: Vec<Template>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Template {
    id: String,
    name: String,
    text: String,
}

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "quick_reply_templates_get" => Operation::new(Read, get),
        "quick_reply_templates_set" => Operation::new(Write, set),
        _ => return None,
    })
}

/// The Web names its signed-in user with `caller_id`; a request without one comes from the
/// local user. Agents and system work never act as a person here.
fn owner(request: &DaemonRequest) -> Result<String, OpError> {
    match request.args.get("by") {
        None => {}
        Some(Value::String(by)) if by.trim() == "user" => {}
        Some(_) => {
            return Err(OpError::new(
                "permission_denied",
                "only a person may read or change their reply templates",
            ));
        }
    }
    Ok(string_arg(request, "caller_id")
        .map(|caller| caller.trim().to_owned())
        .filter(|caller| !caller.is_empty())
        .unwrap_or_else(|| LOCAL_OWNER.to_owned()))
}

fn path(home: &HomeLayout) -> PathBuf {
    home.root().join("state/quick_reply_templates.json")
}

/// A missing file is an empty list; an unreadable one is an error, never an empty list that a
/// save would then write over.
fn load(home: &HomeLayout) -> Result<Document, OpError> {
    let document: Document = match fs::read_json(&path(home)) {
        Ok(document) => document,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Document::default()),
        Err(error) => {
            return Err(OpError::new(
                "quick_reply_templates_invalid",
                format!("the saved reply templates cannot be read: {error}"),
            ));
        }
    };
    if document.v != VERSION
        || document
            .owners
            .values()
            .any(|list| validate(&list.templates).is_err())
    {
        return Err(OpError::new(
            "quick_reply_templates_invalid",
            "the saved reply templates are not in a known form",
        ));
    }
    Ok(document)
}

fn view(list: Option<&OwnerTemplates>) -> OpResult {
    let empty = OwnerTemplates::default();
    let list = list.unwrap_or(&empty);
    object(json!({
        "templates": list.templates,
        "revision": list.revision,
        "updated_at": list.updated_at,
        "limit": MAX_TEMPLATES,
    }))
}

fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let owner = owner(request)?;
    let document = load(home)?;
    view(document.owners.get(&owner))
}

/// Replaces the caller's whole list, in display order. `if_revision` is the revision the
/// caller last read, so a save from another browser in between is reported, not overwritten.
fn set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let owner = owner(request)?;
    let expected = request
        .args
        .get("if_revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| OpError::new("invalid_args", "if_revision must be a revision number"))?;
    let templates = parse(request.args.get("templates"))?;
    let mut document = load(home)?;
    let current = document.owners.get(&owner);
    let revision = current.map_or(0, |list| list.revision);
    if revision != expected {
        let mut error = OpError::new(
            "quick_reply_templates_conflict",
            "the reply templates were changed elsewhere; reload them before saving",
        );
        error.details.insert("revision".into(), json!(revision));
        return Err(error);
    }
    if current.is_some_and(|list| list.templates == templates) {
        return view(current);
    }
    // An emptied list keeps its entry, so its revision still moves on and a browser that read
    // the old list cannot save over the change.
    document.owners.insert(
        owner.clone(),
        OwnerTemplates {
            revision: revision + 1,
            updated_at: utc_now(),
            templates,
        },
    );
    fs::write_json_committed(&path(home), &document).map_err(OpError::io)?;
    view(document.owners.get(&owner))
}

fn parse(value: Option<&Value>) -> Result<Vec<Template>, OpError> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "templates must be a list"))?;
    let mut templates = Vec::with_capacity(items.len());
    for item in items {
        let item = item
            .as_object()
            .ok_or_else(|| OpError::new("invalid_args", "each template must be an object"))?;
        let string_of = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
        let id = match item.get("id") {
            None | Some(Value::Null) => new_id(),
            Some(Value::String(id)) => id.clone(),
            Some(_) => {
                return Err(OpError::new(
                    "invalid_args",
                    "a template id must be a string",
                ));
            }
        };
        // The name is a label, so stray spaces go; the text is sent or inserted exactly as written.
        templates.push(Template {
            id,
            name: string_of("name").trim().to_owned(),
            text: string_of("text").to_owned(),
        });
    }
    validate(&templates).map_err(|message| OpError::new("invalid_args", message))?;
    Ok(templates)
}

fn validate(templates: &[Template]) -> Result<(), String> {
    if templates.len() > MAX_TEMPLATES {
        return Err(format!("at most {MAX_TEMPLATES} reply templates"));
    }
    let mut ids = HashSet::new();
    for template in templates {
        let id_ok = template.id.chars().count() <= MAX_ID_CHARS
            && !template.id.is_empty()
            && template
                .id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
        if !id_ok || !ids.insert(template.id.as_str()) {
            return Err("template ids must be unique letters, digits, '_' or '-'".into());
        }
        let name_chars = template.name.chars().count();
        if name_chars == 0 || name_chars > MAX_NAME_CHARS || template.name.contains(['\n', '\r']) {
            return Err(format!(
                "a template name is 1 to {MAX_NAME_CHARS} characters on one line"
            ));
        }
        if template.text.trim().is_empty() || template.text.chars().count() > MAX_TEXT_CHARS {
            return Err(format!(
                "a template text is at most {MAX_TEXT_CHARS} characters and not only white space"
            ));
        }
    }
    Ok(())
}

fn new_id() -> String {
    format!("qr_{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, HomeLayout) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        (temp, home)
    }

    fn request(op: &str, args: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().expect("object arguments"),
        }
    }

    fn save(home: &HomeLayout, args: Value) -> OpResult {
        set(home, &request("quick_reply_templates_set", args))
    }

    fn read(home: &HomeLayout, args: Value) -> OpResult {
        get(home, &request("quick_reply_templates_get", args))
    }

    #[test]
    fn each_person_keeps_their_own_list_and_agents_are_refused() {
        let (_temp, home) = home();
        let alice = json!([{"name":"Go","text":"Go ahead."}]);
        let local = json!([{"name":"Why","text":"Why this way?"}]);
        save(
            &home,
            json!({"by":"user","caller_id":"alice","if_revision":0,"templates":alice}),
        )
        .expect("alice saves");
        save(
            &home,
            json!({"by":"user","if_revision":0,"templates":local}),
        )
        .expect("local saves");

        let alice_list = read(&home, json!({"by":"user","caller_id":"alice"})).expect("alice");
        assert_eq!(alice_list["templates"][0]["text"], "Go ahead.");
        assert_eq!(alice_list["revision"], 1);
        assert_eq!(alice_list["limit"], MAX_TEMPLATES);
        let local_list = read(&home, json!({"by":"user","caller_id":"local"})).expect("local");
        assert_eq!(local_list["templates"][0]["text"], "Why this way?");
        let bob = read(&home, json!({"by":"user","caller_id":"bob"})).expect("bob");
        assert_eq!(bob["templates"], json!([]));
        assert_eq!(bob["revision"], 0);

        for by in ["peer1", "system"] {
            let error = read(&home, json!({"by":by})).expect_err("refused");
            assert_eq!(error.code, "permission_denied");
            let error =
                save(&home, json!({"by":by,"if_revision":1,"templates":[]})).expect_err("refused");
            assert_eq!(error.code, "permission_denied");
        }
    }

    #[test]
    fn a_save_needs_the_revision_last_read() {
        let (_temp, home) = home();
        let first = json!([{"id":"a","name":"One","text":"First."}]);
        let saved = save(&home, json!({"if_revision":0,"templates":first})).expect("first save");
        assert_eq!(saved["revision"], 1);

        let stale = save(&home, json!({"if_revision":0,"templates":[]})).expect_err("stale");
        assert_eq!(stale.code, "quick_reply_templates_conflict");
        assert_eq!(stale.details["revision"], 1);

        let unchanged = save(&home, json!({"if_revision":1,"templates":first})).expect("same list");
        assert_eq!(
            unchanged["revision"], 1,
            "an unchanged list is not a new revision"
        );

        let emptied = save(&home, json!({"if_revision":1,"templates":[]})).expect("emptied");
        assert_eq!(emptied["revision"], 2);
        assert_eq!(emptied["templates"], json!([]));
        let after = save(&home, json!({"if_revision":1,"templates":first})).expect_err("stale");
        assert_eq!(after.code, "quick_reply_templates_conflict");

        let missing = save(&home, json!({"templates":[]})).expect_err("no revision");
        assert_eq!(missing.code, "invalid_args");
    }

    #[test]
    fn names_are_trimmed_texts_kept_as_written_and_bounded() {
        let (_temp, home) = home();
        let written = "\n  Come back to this later:\n```\n    indented code\n```\n\n";
        let saved = save(
            &home,
            json!({"if_revision":0,"templates":[
                {"name":"  Later  ","text":written},
                {"id":"keep-1","name":"Now","text":"Do it now."}
            ]}),
        )
        .expect("save");
        let templates = saved["templates"].as_array().expect("templates");
        assert_eq!(templates[0]["name"], "Later");
        assert_eq!(templates[0]["text"], written);
        assert!(templates[0]["id"].as_str().expect("id").starts_with("qr_"));
        assert_eq!(templates[1]["id"], "keep-1");
        let reread = read(&home, json!({})).expect("read back");
        assert_eq!(
            reread["templates"][0]["text"], written,
            "leading and trailing line breaks and indentation survive the store"
        );

        let too_many = (0..=MAX_TEMPLATES)
            .map(|index| json!({"name":format!("T{index}"),"text":"x"}))
            .collect::<Vec<_>>();
        let long_name = "n".repeat(MAX_NAME_CHARS + 1);
        let long_text = "t".repeat(MAX_TEXT_CHARS + 1);
        let long_with_space = format!(" {}", "t".repeat(MAX_TEXT_CHARS));
        for templates in [
            json!(too_many),
            json!([{"name":"Two\nlines","text":"x"}]),
            json!([{"name":long_name,"text":"x"}]),
            json!([{"name":"Empty","text":" \n\t "}]),
            json!([{"name":"Long","text":long_text}]),
            json!([{"name":"Spaces count","text":long_with_space}]),
            json!([{"id":"","name":"A","text":"x"}]),
            json!([{"id":"same","name":"A","text":"x"},{"id":"same","name":"B","text":"y"}]),
            json!([{"id":"no spaces","name":"A","text":"x"}]),
            json!({"name":"not a list"}),
        ] {
            let error =
                save(&home, json!({"if_revision":1,"templates":templates})).expect_err("rejected");
            assert_eq!(error.code, "invalid_args", "{error:?}");
        }
        let within = save(
            &home,
            json!({"if_revision":1,"templates":[{"name":"n".repeat(MAX_NAME_CHARS),"text":"t".repeat(MAX_TEXT_CHARS)}]}),
        )
        .expect("limits are inclusive");
        assert_eq!(within["revision"], 2);
    }

    #[test]
    fn an_unreadable_file_is_reported_and_never_saved_over() {
        let (_temp, home) = home();
        let file = path(&home);
        std::fs::create_dir_all(file.parent().expect("state directory")).expect("state dir");
        std::fs::write(&file, b"{ not json").expect("damaged file");

        let error = read(&home, json!({})).expect_err("unreadable");
        assert_eq!(error.code, "quick_reply_templates_invalid");
        let error = save(&home, json!({"if_revision":0,"templates":[]})).expect_err("unreadable");
        assert_eq!(error.code, "quick_reply_templates_invalid");
        assert_eq!(std::fs::read(&file).expect("file"), b"{ not json");
    }
}
