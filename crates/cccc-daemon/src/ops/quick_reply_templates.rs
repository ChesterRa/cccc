//! Reply templates a person keeps for the Web: a short name shown in menus and the text that is
//! sent from a message's quick replies or inserted into the composer. Each access identity has
//! one list, shared by every browser and device it signs in from; agents cannot read or change it.
//! Default replies and personal templates form one ordered list. Default text stays in the Web;
//! only personal overrides are stored. Hidden overrides from earlier records are retained until
//! the person explicitly restores that default.

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
const MAX_TEMPLATES: usize = 24;
const MAX_NAME_CHARS: usize = 40;
const MAX_TEXT_CHARS: usize = 10_000;
const MAX_ID_CHARS: usize = 64;
/// The Web's built-in replies, which a person may rename, reword or hide.
const BUILTIN_KINDS: [&str; 4] = ["proceed", "why", "rethink", "choices"];
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
    /// Changes to built-in replies by kind. Left out of the file while empty, so a record
    /// without changes keeps the shape earlier versions wrote.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    builtins: BTreeMap<String, BuiltinChange>,
    /// Missing in earlier records: defaults first, then the saved personal order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Template {
    id: String,
    name: String,
    text: String,
}

/// A person's change to one built-in reply. A field left out keeps the built-in value, which
/// the Web shows in the interface language.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BuiltinChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
}

/// A visible reply. Default identity is separate from personal IDs; optional fields are
/// overrides, not translated defaults. The array order is the complete display order.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum Reply {
    Builtin {
        builtin: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Personal(Template),
}

impl Reply {
    fn key(&self) -> String {
        match self {
            Self::Builtin { builtin, .. } => format!("builtin:{builtin}"),
            Self::Personal(template) => format!("custom:{}", template.id),
        }
    }
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

/// A missing file has no personal state; an unreadable one is an error, never a default
/// document that a save would then write over.
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
            .any(|list| validate_owner(list).is_err())
    {
        return Err(OpError::new(
            "quick_reply_templates_invalid",
            "the saved reply templates are not in a known form",
        ));
    }
    Ok(document)
}

fn visible_replies(list: &OwnerTemplates) -> Vec<Reply> {
    let mut replies = Vec::new();
    for kind in BUILTIN_KINDS {
        let change = list.builtins.get(kind);
        if change.is_some_and(|change| change.hidden) {
            continue;
        }
        replies.push(Reply::Builtin {
            builtin: kind.into(),
            name: change.and_then(|change| change.name.clone()),
            text: change.and_then(|change| change.text.clone()),
        });
    }
    replies.extend(list.templates.iter().cloned().map(Reply::Personal));
    replies
}

fn ordered_replies(list: &OwnerTemplates) -> Result<Vec<Reply>, String> {
    let replies = visible_replies(list);
    let Some(order) = &list.order else {
        return Ok(replies);
    };
    let mut by_key: BTreeMap<_, _> = replies
        .into_iter()
        .map(|reply| (reply.key(), reply))
        .collect();
    let mut ordered = Vec::with_capacity(order.len());
    for key in order {
        ordered.push(
            by_key
                .remove(key)
                .ok_or("reply order has an unknown or repeated member")?,
        );
    }
    if !by_key.is_empty() {
        return Err("reply order must include every visible member".into());
    }
    Ok(ordered)
}

fn validate_owner(list: &OwnerTemplates) -> Result<(), String> {
    validate(&list.templates)?;
    validate_builtins(&list.builtins)?;
    let replies = ordered_replies(list)?;
    if replies.len() > MAX_TEMPLATES {
        return Err(format!("at most {MAX_TEMPLATES} replies"));
    }
    Ok(())
}

fn view(list: Option<&OwnerTemplates>) -> OpResult {
    let empty = OwnerTemplates::default();
    let list = list.unwrap_or(&empty);
    let replies = ordered_replies(list)
        .map_err(|message| OpError::new("quick_reply_templates_invalid", message))?;
    object(json!({
        "replies": replies,
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

/// Replace the whole visible list over the revision that was read. Missing defaults are
/// hidden while keeping their stored overrides; explicitly present defaults replace those
/// overrides. Thus adding a bare default restores it without changing any other reply.
fn set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let owner = owner(request)?;
    let expected = request
        .args
        .get("if_revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| OpError::new("invalid_args", "if_revision must be a revision number"))?;
    let replies = parse_replies(request.args.get("replies"))?;
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
    let defaults = OwnerTemplates::default();
    let previous = current.unwrap_or(&defaults);
    let mut next = previous.clone();
    next.templates.clear();
    for kind in BUILTIN_KINDS {
        next.builtins.entry(kind.into()).or_default().hidden = true;
    }
    for reply in &replies {
        match reply {
            Reply::Personal(template) => next.templates.push(template.clone()),
            Reply::Builtin {
                builtin,
                name,
                text,
            } => {
                let change = BuiltinChange {
                    name: name.clone(),
                    text: text.clone(),
                    hidden: false,
                };
                if change == BuiltinChange::default() {
                    next.builtins.remove(builtin);
                } else {
                    next.builtins.insert(builtin.clone(), change);
                }
            }
        }
    }
    next.order = Some(replies.iter().map(Reply::key).collect());
    validate_owner(&next).map_err(|message| OpError::new("invalid_args", message))?;
    // A read or unchanged save never rewrites an older record merely to add explicit order.
    let previous_replies = ordered_replies(previous)
        .map_err(|message| OpError::new("quick_reply_templates_invalid", message))?;
    if previous.templates == next.templates
        && previous.builtins == next.builtins
        && previous_replies == replies
    {
        return view(current);
    }
    next.revision = revision + 1;
    next.updated_at = utc_now();
    document.owners.insert(owner.clone(), next);
    fs::write_json_committed(&path(home), &document).map_err(OpError::io)?;
    view(document.owners.get(&owner))
}

fn parse_replies(value: Option<&Value>) -> Result<Vec<Reply>, OpError> {
    let invalid = |message| OpError::new("invalid_args", message);
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("replies must be a list"))?;
    if items.len() > MAX_TEMPLATES {
        return Err(invalid("at most 24 replies"));
    }
    let mut replies = Vec::with_capacity(items.len());
    let mut keys = HashSet::new();
    for item in items {
        let item = item
            .as_object()
            .ok_or_else(|| invalid("each reply must be an object"))?;
        let reply = if let Some(kind) = item.get("builtin") {
            if item
                .keys()
                .any(|key| !matches!(key.as_str(), "builtin" | "name" | "text"))
            {
                return Err(invalid(
                    "a default reply only accepts builtin, name and text",
                ));
            }
            let kind = kind
                .as_str()
                .ok_or_else(|| invalid("builtin must be a reply kind"))?;
            check_kind(kind).map_err(|message| OpError::new("invalid_args", message))?;
            let field = |key| -> Result<Option<String>, OpError> {
                match item.get(key) {
                    None => Ok(None),
                    Some(Value::String(value)) => Ok(Some(value.clone())),
                    Some(_) => Err(invalid("a reply name or text must be a string")),
                }
            };
            let name = field("name")?.map(|name| name.trim().to_owned());
            let text = field("text")?;
            if let Some(name) = &name {
                check_name(name).map_err(|message| OpError::new("invalid_args", message))?;
            }
            if let Some(text) = &text {
                check_text(text).map_err(|message| OpError::new("invalid_args", message))?;
            }
            Reply::Builtin {
                builtin: kind.into(),
                name,
                text,
            }
        } else {
            if item
                .keys()
                .any(|key| !matches!(key.as_str(), "id" | "name" | "text"))
            {
                return Err(invalid("a personal reply only accepts id, name and text"));
            }
            let id = match item.get("id") {
                None => new_id(),
                Some(Value::String(id)) => id.clone(),
                Some(_) => return Err(invalid("a reply id must be a string")),
            };
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("a personal reply needs a name"))?
                .trim()
                .to_owned();
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("a personal reply needs text"))?
                .to_owned();
            let template = Template { id, name, text };
            validate(std::slice::from_ref(&template))
                .map_err(|message| OpError::new("invalid_args", message))?;
            Reply::Personal(template)
        };
        if !keys.insert(reply.key()) {
            return Err(invalid("reply identities must be unique"));
        }
        replies.push(reply);
    }
    Ok(replies)
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
        check_name(&template.name)?;
        check_text(&template.text)?;
    }
    Ok(())
}

/// Stored default overrides follow the same field rules; visible replies share one limit.
fn validate_builtins(builtins: &BTreeMap<String, BuiltinChange>) -> Result<(), String> {
    for (kind, change) in builtins {
        check_kind(kind)?;
        if *change == BuiltinChange::default() {
            return Err(format!(
                "the change to built-in reply {kind:?} changes nothing"
            ));
        }
        if let Some(name) = &change.name {
            check_name(name)?;
        }
        if let Some(text) = &change.text {
            check_text(text)?;
        }
    }
    Ok(())
}

fn check_kind(kind: &str) -> Result<(), String> {
    if BUILTIN_KINDS.contains(&kind) {
        return Ok(());
    }
    Err(format!(
        "{kind:?} is not a built-in reply; use one of {}",
        BUILTIN_KINDS.join(", ")
    ))
}

fn check_name(name: &str) -> Result<(), String> {
    let name_chars = name.chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS || name.contains(['\n', '\r']) {
        return Err(format!(
            "a template name is 1 to {MAX_NAME_CHARS} characters on one line"
        ));
    }
    Ok(())
}

fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!(
            "a template text is at most {MAX_TEXT_CHARS} characters and not only white space"
        ));
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

    fn defaults() -> Value {
        json!([{"builtin":"proceed"},{"builtin":"why"},{"builtin":"rethink"},{"builtin":"choices"}])
    }

    #[test]
    fn reading_defaults_never_creates_a_store_and_empty_saves_stay_empty() {
        let (_temp, home) = home();
        let first = read(&home, json!({})).expect("defaults");
        assert_eq!(first["replies"], defaults());
        assert_eq!(first["revision"], 0);
        assert_eq!(first["updated_at"], "");
        assert_eq!(first["limit"], 24);
        assert!(!path(&home).exists());
        let unchanged = save(&home, json!({"if_revision":0,"replies":first["replies"]}))
            .expect("save defaults unchanged");
        assert_eq!(unchanged, first);
        assert!(!path(&home).exists(), "unchanged defaults need no file");

        save(
            &home,
            json!({"caller_id":"alice","if_revision":0,"replies":[]}),
        )
        .expect("another owner creates the file");
        let bytes = std::fs::read(path(&home)).expect("other owner's record");
        let unchanged = save(&home, json!({"if_revision":0,"replies":first["replies"]}))
            .expect("absent owner stays absent");
        assert_eq!(unchanged, first);
        assert_eq!(std::fs::read(path(&home)).expect("same file"), bytes);
        let saved = save(&home, json!({"if_revision":0,"replies":[]})).expect("delete all");
        assert_eq!(saved["replies"], json!([]));
        assert_eq!(
            read(&home, json!({})).expect("empty remains")["replies"],
            json!([])
        );
        assert_eq!(
            save(&home, json!({"if_revision":1,"replies":[]})).expect("same")["revision"],
            1
        );
    }

    #[test]
    fn each_person_keeps_their_own_list_and_agents_are_refused() {
        let (_temp, home) = home();
        let alice = json!([{"name":"Go","text":"Go ahead."}]);
        let local = json!([{"name":"Why","text":"Why this way?"}]);
        save(
            &home,
            json!({"by":"user","caller_id":"alice","if_revision":0,"replies":alice}),
        )
        .expect("alice saves");
        save(&home, json!({"by":"user","if_revision":0,"replies":local})).expect("local saves");

        let alice_list = read(&home, json!({"by":"user","caller_id":"alice"})).expect("alice");
        assert_eq!(alice_list["replies"][0]["text"], "Go ahead.");
        assert_eq!(alice_list["revision"], 1);
        assert_eq!(alice_list["limit"], MAX_TEMPLATES);
        let local_list = read(&home, json!({"by":"user","caller_id":"local"})).expect("local");
        assert_eq!(local_list["replies"][0]["text"], "Why this way?");
        let bob = read(&home, json!({"by":"user","caller_id":"bob"})).expect("bob");
        assert_eq!(bob["replies"], defaults());
        assert_eq!(bob["revision"], 0);

        for by in ["peer1", "system"] {
            let error = read(&home, json!({"by":by})).expect_err("refused");
            assert_eq!(error.code, "permission_denied");
            let error =
                save(&home, json!({"by":by,"if_revision":1,"replies":[]})).expect_err("refused");
            assert_eq!(error.code, "permission_denied");
        }
    }

    #[test]
    fn a_save_needs_the_revision_last_read() {
        let (_temp, home) = home();
        let first = json!([{"id":"a","name":"One","text":"First."}]);
        let saved = save(&home, json!({"if_revision":0,"replies":first})).expect("first save");
        assert_eq!(saved["revision"], 1);

        let stale = save(&home, json!({"if_revision":0,"replies":[]})).expect_err("stale");
        assert_eq!(stale.code, "quick_reply_templates_conflict");
        assert_eq!(stale.details["revision"], 1);

        let unchanged = save(&home, json!({"if_revision":1,"replies":first})).expect("same list");
        assert_eq!(
            unchanged["revision"], 1,
            "an unchanged list is not a new revision"
        );

        let emptied = save(&home, json!({"if_revision":1,"replies":[]})).expect("emptied");
        assert_eq!(emptied["revision"], 2);
        assert_eq!(emptied["replies"], json!([]));
        let after = save(&home, json!({"if_revision":1,"replies":first})).expect_err("stale");
        assert_eq!(after.code, "quick_reply_templates_conflict");

        let missing = save(&home, json!({"replies":[]})).expect_err("no revision");
        assert_eq!(missing.code, "invalid_args");
    }

    #[test]
    fn names_are_trimmed_texts_kept_as_written_and_bounded() {
        let (_temp, home) = home();
        let written = "\n  Come back to this later:\n```\n    indented code\n```\n\n";
        let saved = save(
            &home,
            json!({"if_revision":0,"replies":[
                {"name":"  Later  ","text":written},
                {"id":"keep-1","name":"Now","text":"Do it now."}
            ]}),
        )
        .expect("save");
        let templates = saved["replies"].as_array().expect("replies");
        assert_eq!(templates[0]["name"], "Later");
        assert_eq!(templates[0]["text"], written);
        assert!(templates[0]["id"].as_str().expect("id").starts_with("qr_"));
        assert_eq!(templates[1]["id"], "keep-1");
        let reread = read(&home, json!({})).expect("read back");
        assert_eq!(
            reread["replies"][0]["text"], written,
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
                save(&home, json!({"if_revision":1,"replies":templates})).expect_err("rejected");
            assert_eq!(error.code, "invalid_args", "{error:?}");
        }
        let within = save(
            &home,
            json!({"if_revision":1,"replies":[{"name":"n".repeat(MAX_NAME_CHARS),"text":"t".repeat(MAX_TEXT_CHARS)}]}),
        )
        .expect("limits are inclusive");
        assert_eq!(within["revision"], 2);
    }

    #[test]
    fn an_unreadable_file_is_reported_and_never_saved_over() {
        let (_temp, home) = home();
        let file = path(&home);
        std::fs::create_dir_all(file.parent().expect("state directory")).expect("state dir");
        for damaged in [
            b"{ not json".as_slice(),
            br#"{"v":1,"owners":{"local":{"revision":1,"updated_at":"","templates":[],"builtins":{"other":{"hidden":true}}}}}"#,
        ] {
            std::fs::write(&file, damaged).expect("damaged file");
            let error = read(&home, json!({})).expect_err("unreadable");
            assert_eq!(error.code, "quick_reply_templates_invalid");
            let error =
                save(&home, json!({"if_revision":1,"replies":[]})).expect_err("unreadable");
            assert_eq!(error.code, "quick_reply_templates_invalid");
            assert_eq!(std::fs::read(&file).expect("file"), damaged);
        }
    }

    fn stored(home: &HomeLayout) -> Value {
        serde_json::from_slice(&std::fs::read(path(home)).expect("file")).expect("stored json")
    }

    fn write_store(home: &HomeLayout, document: &Value) -> Vec<u8> {
        let file = path(home);
        std::fs::create_dir_all(file.parent().expect("state directory")).expect("state dir");
        let bytes = serde_json::to_vec(document).expect("JSON");
        std::fs::write(file, &bytes).expect("write record");
        bytes
    }

    #[test]
    fn mixed_order_restoration_deletion_and_adding_back_have_one_saved_order() {
        let (_temp, home) = home();
        let raw = "\n  Keep this indentation.\n";
        let initial = json!([
            {"id":"one","name":"Mine","text":"One"},
            {"builtin":"choices","text":raw},
            {"builtin":"proceed","name":"  Go  "},
            {"id":"two","name":"Later","text":"Two"}
        ]);
        let saved = save(&home, json!({"if_revision":0,"replies":initial})).expect("mixed");
        let expected = json!([
            {"id":"one","name":"Mine","text":"One"},
            {"builtin":"choices","text":raw},
            {"builtin":"proceed","name":"Go"},
            {"id":"two","name":"Later","text":"Two"}
        ]);
        assert_eq!(saved["replies"], expected);
        assert_eq!(read(&home, json!({})).expect("reread")["replies"], expected);
        assert_eq!(
            stored(&home)["owners"]["local"]["order"],
            json!([
                "custom:one",
                "builtin:choices",
                "builtin:proceed",
                "custom:two"
            ])
        );
        let reordered = json!([
            {"builtin":"proceed"},
            {"id":"two","name":"Later","text":"Two"},
            {"id":"one","name":"Mine","text":"One"}
        ]);
        let restored =
            save(&home, json!({"if_revision":1,"replies":reordered})).expect("restore and move");
        assert_eq!(restored["replies"], reordered);
        // A missing default keeps its override, even when another default is restored.
        assert_eq!(
            stored(&home)["owners"]["local"]["builtins"]["choices"],
            json!({"text":raw,"hidden":true})
        );
        let mut added = reordered.as_array().expect("list").clone();
        added.push(json!({"builtin":"choices"}));
        let added =
            save(&home, json!({"if_revision":2,"replies":added})).expect("add default at end");
        assert_eq!(added["replies"][3], json!({"builtin":"choices"}));
        assert_eq!(added["replies"][1], reordered[1]);
        assert!(
            stored(&home)["owners"]["local"]["builtins"]
                .get("choices")
                .is_none()
        );
    }

    #[test]
    fn trial_records_keep_all_owners_and_hidden_overrides_without_read_or_noop_writes() {
        let (_temp, home) = home();
        let custom = (0..20)
            .map(|i| json!({"id":format!("old_{i}"),"name":format!("Name {i}"),"text":"\n x \n"}))
            .collect::<Vec<_>>();
        let original = json!({"v":1,"owners":{
            "local":{"revision":7,"updated_at":"then","templates":custom},
            "alice":{"revision":3,"updated_at":"earlier","templates":[{"id":"alice_id","name":"Alice","text":" A "}],
                "builtins":{"proceed":{"name":"Go"},"why":{"name":"Hidden","text":"Secret draft","hidden":true}}}
        }});
        let bytes = write_store(&home, &original);
        let local = read(&home, json!({})).expect("local trial list");
        assert_eq!(local["replies"].as_array().expect("list").len(), 24);
        assert_eq!(local["replies"][4]["id"], "old_0");
        let alice = read(&home, json!({"caller_id":"alice"})).expect("alice trial list");
        assert_eq!(alice["revision"], 3);
        assert_eq!(
            alice["replies"],
            json!([
                {"builtin":"proceed","name":"Go"},{"builtin":"rethink"},{"builtin":"choices"},
                {"id":"alice_id","name":"Alice","text":" A "}
            ])
        );
        let same = save(&home, json!({"if_revision":7,"replies":local["replies"]}))
            .expect("no-op on old record");
        assert_eq!(same["revision"], 7);
        assert_eq!(std::fs::read(path(&home)).expect("unchanged file"), bytes);
        let mut replies = alice["replies"].as_array().expect("list").clone();
        replies.swap(0, 3);
        save(
            &home,
            json!({"caller_id":"alice","if_revision":3,"replies":replies}),
        )
        .expect("move");
        let after = stored(&home);
        assert_eq!(after["owners"]["local"], original["owners"]["local"]);
        assert_eq!(
            after["owners"]["alice"]["builtins"]["why"],
            original["owners"]["alice"]["builtins"]["why"]
        );
        assert_eq!(
            read(&home, json!({"caller_id":"alice"})).expect("reload")["replies"],
            json!(replies)
        );
    }

    #[test]
    fn one_capacity_includes_defaults_and_deletion_frees_a_place() {
        let (_temp, home) = home();
        let mut full = defaults().as_array().expect("defaults").clone();
        full.extend((0..20).map(|i| json!({"id":format!("r_{i}"),"name":"Mine","text":"x"})));
        save(&home, json!({"if_revision":0,"replies":full})).expect("24 entries");
        full.push(json!({"id":"extra","name":"New","text":"x"}));
        assert_eq!(
            save(&home, json!({"if_revision":1,"replies":full}))
                .expect_err("25")
                .code,
            "invalid_args"
        );
        full.remove(0);
        let replaced =
            save(&home, json!({"if_revision":1,"replies":full})).expect("free one default slot");
        assert_eq!(replaced["replies"].as_array().expect("list").len(), 24);
        assert_eq!(replaced["replies"][23]["id"], "extra");
    }

    #[test]
    fn malformed_replies_never_clear_existing_content() {
        let (_temp, home) = home();
        let good = json!([{"builtin":"proceed","name":"Go"}]);
        save(&home, json!({"if_revision":0,"replies":good})).expect("initial");
        let bytes = std::fs::read(path(&home)).expect("record");
        for replies in [
            json!(null),
            json!({}),
            json!([{}]),
            json!([{"builtin":"bogus"}]),
            json!([{"builtin":"bogus","hidden":false}]),
            json!([{"builtin":null}]),
            json!([{"builtin":"why","id":"x"}]),
            json!([{"builtin":"why","hidden":true}]),
            json!([{"builtin":"why","name":null}]),
            json!([{"builtin":"why","name":"  "}]),
            json!([{"builtin":"why","text":" \n "}]),
            json!([{"builtin":"why","emoji":"?"}]),
            json!([{"builtin":"why"},{"builtin":"why"}]),
            json!([{"id":null,"name":"Mine","text":"x"}]),
            json!([{"name":"Mine","text":"x","hidden":false}]),
        ] {
            assert_eq!(
                save(&home, json!({"if_revision":1,"replies":replies}))
                    .expect_err("bad reply")
                    .code,
                "invalid_args"
            );
            assert_eq!(std::fs::read(path(&home)).expect("unchanged"), bytes);
        }
        assert_eq!(
            save(&home, json!({"if_revision":1,"templates":[]}))
                .expect_err("missing replies")
                .code,
            "invalid_args"
        );
        assert_eq!(read(&home, json!({})).expect("read")["replies"], good);
        assert_eq!(std::fs::read(path(&home)).expect("unchanged"), bytes);
    }

    #[test]
    fn an_invalid_order_or_another_bad_owner_prevents_overwriting_the_file() {
        let (_temp, home) = home();
        for order in [
            json!([]),
            json!(["builtin:proceed", "builtin:proceed"]),
            json!(["custom:missing"]),
            json!(["builtin:bogus"]),
        ] {
            let bytes = write_store(
                &home,
                &json!({"v":1,"owners":{
                    "alice":{"revision":1,"updated_at":"then","templates":[],"order":order}
                }}),
            );
            assert_eq!(
                read(&home, json!({})).expect_err("bad other owner").code,
                "quick_reply_templates_invalid"
            );
            assert_eq!(
                save(&home, json!({"if_revision":0,"replies":[]}))
                    .expect_err("do not overwrite")
                    .code,
                "quick_reply_templates_invalid"
            );
            assert_eq!(std::fs::read(path(&home)).expect("unchanged"), bytes);
        }
    }
}
