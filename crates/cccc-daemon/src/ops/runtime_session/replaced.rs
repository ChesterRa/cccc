//! A launch that starts a new conversation overwrites the receipt of the one
//! saved before. Note that on the new receipt: the member came back without
//! its saved conversation, and nothing else would record it.
use super::resume_enabled;
use cccc_contracts::{Event, utc_now};
use cccc_core::codex_voice_settings::LaunchParts;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const KEY: &str = "replaced";

/// Notes this daemon wrote and has not announced yet, by receipt. A note is
/// announced once its launch is attached; one lost with the daemon stays on
/// the receipt and in the listing.
fn unannounced() -> &'static Mutex<HashMap<PathBuf, Value>> {
    static UNANNOUNCED: OnceLock<Mutex<HashMap<PathBuf, Value>>> = OnceLock::new();
    UNANNOUNCED.get_or_init(Mutex::default)
}

/// The receipt fields a `prepare` compares, each with the reason reported when
/// it differs. A receipt holds a field at its top level or inside `identity`.
const COMPARED: [(&str, &str); 8] = [
    ("v", "record_version"),
    ("workspace_path", "workspace"),
    ("command_fingerprint", "command"),
    ("model", "model"),
    ("identity_fingerprint", "launch_identity"),
    ("launch_fingerprint", "launch_identity"),
    ("provider_home", "launch_identity"),
    // Antigravity's ACP distribution, not a format of the receipt.
    ("version", "launch_identity"),
];

/// A receipt may split its launch fingerprint into these parts, beside the
/// fields a `prepare` compares. Each part has the reason reported when it is
/// the one that differs.
const PARTS: &str = "launch_parts";
const PART_REASONS: [(&str, &str); 3] = [
    ("path", "launch_path"),
    ("cccc_cli", "cccc_cli"),
    ("rest", "launch_identity"),
];

/// Write the receipt of the conversation a launch now uses. `path` still holds
/// the receipt it replaces.
pub(super) fn write_recorded(path: &Path, receipt: Value) -> std::io::Result<()> {
    write_noting(path, receipt, resume_enabled())
}

fn write_noting(path: &Path, mut receipt: Value, resume: bool) -> std::io::Result<()> {
    // With resume disabled, a fresh conversation is what the operator asked for.
    let noted = resume
        && cccc_core::fs::read_json::<Value>(path).is_ok_and(|saved| note(&saved, &mut receipt));
    cccc_core::fs::write_json(path, &receipt)?;
    if noted && let Ok(mut unannounced) = unannounced().lock() {
        unannounced.insert(path.to_path_buf(), receipt[KEY].clone());
    }
    Ok(())
}

/// Returns whether this launch left a saved conversation behind.
fn note(saved: &Value, receipt: &mut Value) -> bool {
    let Some(previous) = conversation(saved) else {
        return false;
    };
    // The same conversation keeps what was noted when it began. So does the
    // successor of one that never received a prompt, which only Pi and ACP
    // receipts can tell: nothing was left behind there.
    if Some(previous) == conversation(receipt) || saved["attempted"] == false {
        if !saved[KEY].is_null() {
            receipt[KEY] = saved[KEY].clone();
        }
        return false;
    }
    // A receipt proves that a conversation was saved, not that it held work.
    receipt[KEY] = json!({
        "at": utc_now(),
        "reasons": reasons(saved, receipt),
        "previous": {
            "runtime": previous.0,
            "transport": previous.1,
            "provider_id": previous.2,
        },
    });
    true
}

/// The note of the receipt at `path`, the first time its launch is attached.
/// The receipt may have moved on since it was noted; only what it still says
/// is announced.
pub(super) fn take_unannounced(path: &Path) -> Option<Value> {
    let noted = unannounced().lock().ok()?.remove(path)?;
    cccc_core::fs::read_json::<Value>(path)
        .is_ok_and(|receipt| receipt[KEY] == noted)
        .then_some(noted)
}

/// Tells the user, in the ledger and on a connected IM, that a member came
/// back without its saved conversation. The Web conversation shows chat
/// messages only; there the member's listing fields carry the same fact.
pub(super) fn notice(group_id: &str, actor_id: &str, noted: &Value) -> Event {
    let changed: Vec<_> = noted["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reason| match reason.as_str()? {
            "runtime" => Some("runtime"),
            "transport" => Some("connection type"),
            "record_version" => Some("CCCC's saved format"),
            "workspace" => Some("workspace"),
            "command" => Some("launch command"),
            "model" => Some("model"),
            "launch_path" => Some("launch PATH"),
            "cccc_cli" => Some("CCCC program location"),
            "launch_identity" => Some("launch settings or environment"),
            _ => None,
        })
        .collect();
    let mut message =
        format!("{actor_id} started a new conversation. The one saved before it was not resumed.");
    if !changed.is_empty() {
        message = format!(
            "{message} Changed since it was saved: {}.",
            changed.join(", ")
        );
    }
    let mut event = Event::new("system.notify", group_id);
    event.by = "system".into();
    // No `target_actor_id` or `actor_id`: either would route this to the member
    // and keep it off IM.
    event.data = json!({
        "kind": "info",
        "title": format!("{actor_id}: saved conversation not resumed"),
        "message": message,
        "to": ["user"],
        "im_visibility": "public",
        "context": {
            "kind": "runtime_session_replaced",
            "member": actor_id,
            "replaced_at": noted["at"],
            "reasons": noted["reasons"],
        },
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    event
}

/// The provider conversation a receipt points at. Native ACP receipts name no
/// runtime or transport: each of those runtimes has its own receipt file.
fn conversation(receipt: &Value) -> Option<(&str, &str, &str)> {
    fn text<'a>(receipt: &'a Value, key: &str) -> &'a str {
        receipt[key].as_str().unwrap_or_default().trim()
    }
    let id = match text(receipt, "provider_thread_id") {
        "" => text(receipt, "provider_session_id"),
        id => id,
    };
    (!id.is_empty()).then(|| (text(receipt, "runtime"), text(receipt, "transport"), id))
}

fn reasons(saved: &Value, receipt: &Value) -> Vec<&'static str> {
    // Under another runtime or transport the remaining fields do not compare.
    let surface: Vec<_> = ["runtime", "transport"]
        .into_iter()
        .filter(|field| saved[*field] != receipt[*field])
        .collect();
    if !surface.is_empty() {
        return surface;
    }
    let mut reasons = Vec::new();
    for (field, reason) in COMPARED {
        if compared(saved, field) == compared(receipt, field) {
            continue;
        }
        let named = match field {
            "identity_fingerprint" | "launch_fingerprint" => changed_parts(saved, receipt),
            _ => None,
        };
        for reason in named.unwrap_or_else(|| vec![reason]) {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
    }
    reasons
}

/// The parts of a changed launch fingerprint that differ. `None` unless both
/// receipts carry a split this build understands and it shows a difference:
/// the caller then reports the fingerprint as a whole.
fn changed_parts(saved: &Value, receipt: &Value) -> Option<Vec<&'static str>> {
    let understood = |receipt: &Value| {
        let parts = &receipt[PARTS];
        parts["v"] == LaunchParts::VERSION
            && PART_REASONS
                .iter()
                .all(|(part, _)| parts[*part].is_string())
    };
    if !understood(saved) || !understood(receipt) {
        return None;
    }
    let changed: Vec<_> = PART_REASONS
        .iter()
        .filter(|(part, _)| saved[PARTS][*part] != receipt[PARTS][*part])
        .map(|(_, reason)| *reason)
        .collect();
    (!changed.is_empty()).then_some(changed)
}

/// Whether this launch differs from the one `saved` recorded only in its
/// launch `PATH`: both splits are this build's and complete, and `path` is the
/// one part that differs. Such a launch may resume the saved conversation; a
/// receipt saved without a split never qualifies.
pub(super) fn only_launch_path_changed(saved: &Value, current: &LaunchParts) -> bool {
    let current = Value::Object([(PARTS.to_owned(), json!(current))].into_iter().collect());
    changed_parts(saved, &current).is_some_and(|changed| changed == ["launch_path"])
}

/// Whether a receipt that matches by `identity` (Pi, native ACP) may resume
/// under this launch: the identities are equal, or both carry a launch
/// fingerprint, differ only in it, and that difference is the launch `PATH`
/// alone. A saved identity without a fingerprint never qualifies.
pub(super) fn identity_resumes(saved: &Value, identity: &Value, current: &LaunchParts) -> bool {
    const FINGERPRINT: &str = "launch_fingerprint";
    fn others(fields: &serde_json::Map<String, Value>) -> impl Iterator<Item = (&String, &Value)> {
        fields
            .iter()
            .filter(|(field, _)| field.as_str() != FINGERPRINT)
    }
    let fingerprinted = |fields: &serde_json::Map<String, Value>| {
        fields
            .get(FINGERPRINT)
            .and_then(Value::as_str)
            .is_some_and(|fingerprint| !fingerprint.is_empty())
    };
    if &saved["identity"] == identity {
        return true;
    }
    let (Some(saved_fields), Some(fields)) = (saved["identity"].as_object(), identity.as_object())
    else {
        return false;
    };
    fingerprinted(saved_fields)
        && fingerprinted(fields)
        && others(saved_fields).eq(others(fields))
        && only_launch_path_changed(saved, current)
}

fn compared<'a>(receipt: &'a Value, field: &str) -> &'a Value {
    match &receipt[field] {
        Value::Null => &receipt["identity"][field],
        value => value,
    }
}

/// Listing fields of a conversation that began in place of a saved one.
pub(super) fn actor_fields(receipt: &Value) -> [(String, Value); 2] {
    let noted = |key: &str, valid: fn(&Value) -> bool| {
        Some(&receipt[KEY][key])
            .filter(|value| valid(value))
            .cloned()
            .unwrap_or(Value::Null)
    };
    [
        (
            "runtime_session_replaced_at".into(),
            noted("at", Value::is_string),
        ),
        (
            "runtime_session_replaced_reasons".into(),
            noted("reasons", Value::is_array),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::runtime_session as receipts;
    use cccc_contracts::ActorRuntime;
    use cccc_core::{GroupStore, HomeLayout};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const FIRST: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND: &str = "22222222-2222-4222-8222-222222222222";
    const THIRD: &str = "33333333-3333-4333-8333-333333333333";

    struct Fixture {
        _temp: tempfile::TempDir,
        home: HomeLayout,
        group: String,
        cwd: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            home.initialize().expect("initialize");
            let group = GroupStore::new(home.clone())
                .expect("store")
                .create("replaced receipt", "")
                .expect("group")
                .group_id;
            let cwd = temp.path().join("workspace");
            std::fs::create_dir(&cwd).expect("workspace");
            Self {
                _temp: temp,
                home,
                group,
                cwd,
            }
        }

        fn path(&self) -> PathBuf {
            receipts::path(&self.home, &self.group, "peer").expect("receipt path")
        }

        fn claude(&self, model: &str, config: &str, id: &str, resumed: bool) {
            let (command, environment) = claude_launch(model, config);
            receipts::record_claude_managed_session(
                &self.home,
                &self.group,
                "peer",
                &self.cwd,
                &command,
                &environment,
                id,
                resumed,
            )
            .expect("record Claude session");
        }

        fn resumes(&self, model: &str, config: &str) -> Option<String> {
            let (command, environment) = claude_launch(model, config);
            receipts::prepare_claude_managed_session(
                &self.home,
                &self.group,
                "peer",
                &self.cwd,
                &command,
                &environment,
            )
            .expect("prepare Claude session")
            .map(|attempt| attempt.session_id)
        }

        fn fields(&self) -> (Value, Value) {
            let fields = receipts::actor_fields(&self.home, &self.group, "peer");
            (
                fields["runtime_session_replaced_at"].clone(),
                fields["runtime_session_replaced_reasons"].clone(),
            )
        }
    }

    fn claude_launch(model: &str, config: &str) -> (Vec<String>, BTreeMap<String, String>) {
        (
            vec!["claude".into(), "--model".into(), model.into()],
            BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), config.into())]),
        )
    }

    fn stored(path: &Path) -> Value {
        cccc_core::fs::read_json(path).expect("stored receipt")
    }

    #[test]
    fn a_changed_launch_identity_is_noted_and_stays_with_the_new_conversation() {
        let fixture = Fixture::new();
        fixture.claude("sonnet", "/config/a", FIRST, false);
        assert_eq!(fixture.fields(), (Value::Null, Value::Null));

        assert_eq!(fixture.resumes("sonnet", "/config/b"), None);
        fixture.claude("sonnet", "/config/b", SECOND, false);
        let noted = stored(&fixture.path())[KEY].clone();
        assert_eq!(noted["reasons"], json!(["launch_identity"]));
        assert_eq!(
            noted["previous"],
            json!({"runtime":"claude","transport":"claude_agent_view","provider_id":FIRST})
        );
        assert_eq!(
            fixture.fields(),
            (noted["at"].clone(), noted["reasons"].clone())
        );
        assert!(noted["at"].is_string());

        // Later starts resume the new conversation, which keeps how it began.
        assert_eq!(
            fixture.resumes("sonnet", "/config/b").as_deref(),
            Some(SECOND)
        );
        fixture.claude("sonnet", "/config/b", SECOND, true);
        assert_eq!(stored(&fixture.path())[KEY], noted);
    }

    #[test]
    fn a_noted_replacement_is_announced_to_the_user_once() {
        let fixture = Fixture::new();
        let ledger = GroupStore::new(fixture.home.clone())
            .expect("store")
            .ledger_path(&fixture.group)
            .expect("ledger path");
        let announce = || receipts::announce_replaced(&fixture.home, &fixture.group, "peer");
        let notices = || {
            cccc_core::ledger::read_all(&ledger)
                .unwrap_or_default()
                .into_iter()
                .filter(|event| event.kind == "system.notify")
                .collect::<Vec<_>>()
        };
        fixture.claude("sonnet", "/config/a", FIRST, false);
        announce();
        assert!(notices().is_empty(), "first start");

        fixture.claude("sonnet", "/config/b", SECOND, false);
        announce();
        // A later start resumes the new conversation and has nothing to add.
        fixture.claude("sonnet", "/config/b", SECOND, true);
        announce();
        let sent = notices();
        assert_eq!(sent.len(), 1);
        let (by, data) = (&sent[0].by, &sent[0].data);
        assert_eq!(by, "system");
        assert_eq!(data["to"], json!(["user"]));
        assert_eq!(data["im_visibility"], "public");
        // Either key would deliver the notice to the member and keep it off IM.
        assert!(!data.contains_key("target_actor_id") && !data.contains_key("actor_id"));
        assert_eq!(
            data["message"],
            "peer started a new conversation. The one saved before it was not resumed. \
             Changed since it was saved: launch settings or environment."
        );
        assert_eq!(data["context"]["member"], "peer");
        assert_eq!(data["context"]["reasons"], json!(["launch_identity"]));
        assert!(!data.values().any(|value| value.to_string().contains(FIRST)));
        // It is the user's to read: no member finds it in its inbox, the returning one included.
        let mut group = GroupStore::new(fixture.home.clone())
            .and_then(|store| store.load(&fixture.group))
            .expect("group");
        for member in ["peer", "other"] {
            group.actors.push(cccc_contracts::Actor::new(member));
            assert!(!cccc_core::inbox::is_for_actor(&group, &sent[0], member));
        }

        // A launch that never attached leaves its note waiting. New session then clears the
        // receipt, and the start after it has nothing saved to leave behind.
        fixture.claude("sonnet", "/config/c", THIRD, false);
        receipts::remove(&fixture.home, &fixture.group, "peer").expect("New session");
        fixture.claude("sonnet", "/config/c", FIRST, false);
        announce();
        assert_eq!(notices().len(), 1);
    }

    #[test]
    fn a_paused_resume_does_not_hide_a_later_change() {
        let fixture = Fixture::new();
        fixture.claude("sonnet", "/config/a", FIRST, false);
        let mut paused = stored(&fixture.path());
        paused["status"] = json!("resume_failed");
        cccc_core::fs::write_json(&fixture.path(), &paused).expect("paused receipt");

        assert_eq!(fixture.resumes("opus", "/config/a"), None);
        fixture.claude("opus", "/config/a", SECOND, false);
        assert_eq!(
            stored(&fixture.path())[KEY]["reasons"],
            json!(["command", "model", "launch_identity"])
        );
    }

    #[test]
    fn nothing_is_noted_without_a_saved_conversation_to_resume() {
        let fixture = Fixture::new();
        fixture.claude("sonnet", "/config/a", FIRST, false);
        assert!(stored(&fixture.path())[KEY].is_null(), "first start");

        receipts::remove(&fixture.home, &fixture.group, "peer").expect("New session");
        fixture.claude("sonnet", "/config/b", SECOND, false);
        assert!(stored(&fixture.path())[KEY].is_null(), "New session");

        let fresh = json!({"v":1,"identity":{},"provider_session_id":"fresh","attempted":false});
        write_noting(&fixture.path(), fresh, false).expect("disabled resume");
        assert!(stored(&fixture.path())[KEY].is_null(), "disabled resume");
    }

    #[test]
    fn a_session_that_never_received_a_prompt_leaves_nothing_behind() {
        let fixture = Fixture::new();
        let path = receipts::native_acp::receipt_path(
            &fixture.home,
            &fixture.group,
            "peer",
            ActorRuntime::Kimi,
        )
        .expect("ACP receipt path");
        let record = |provider_home: &str, id: &str| {
            let (identity, parts) = receipts::native_acp::launch(
                &fixture.cwd,
                &["kimi".to_owned()],
                &BTreeMap::from([("KIMI_HOME".to_owned(), provider_home.to_owned())]),
            );
            receipts::native_acp::record(&path, &identity, &parts, id, false)
                .expect("record ACP session");
        };
        record("/kimi/a", "acp-1");
        record("/kimi/b", "acp-2");
        assert!(stored(&path)[KEY].is_null());

        receipts::native_acp::PromptReceipt::actor(path.clone(), "acp-2".into(), false)
            .mark_attempted()
            .expect("first prompt");
        record("/kimi/c", "acp-3");
        let noted = stored(&path)[KEY].clone();
        assert_eq!(noted["reasons"], json!(["launch_identity"]));
        assert_eq!(noted["previous"]["provider_id"], "acp-2");

        // acp-3 was never prompted, so the conversation left behind is still acp-2.
        record("/kimi/d", "acp-4");
        assert_eq!(stored(&path)[KEY], noted);
    }

    #[test]
    fn another_runtime_or_an_unchanged_identity_names_no_setting() {
        let fixture = Fixture::new();
        fixture.claude("sonnet", "/config/a", FIRST, false);
        let (pi, parts) = receipts::pi::launch(&fixture.cwd, &["pi".to_owned()], &BTreeMap::new())
            .expect("Pi launch identity");
        receipts::pi::record(
            &fixture.path(),
            &pi,
            parts.as_ref(),
            SECOND,
            &fixture.cwd.join("session.jsonl"),
            false,
        )
        .expect("record Pi session");
        assert_eq!(
            stored(&fixture.path())[KEY]["reasons"],
            json!(["runtime", "transport"])
        );

        let other = Fixture::new();
        for thread in ["thread-1", "thread-2"] {
            receipts::record_codex_app_thread(
                &other.home,
                &other.group,
                "peer",
                &other.cwd,
                &["codex".to_owned(), "app-server".to_owned()],
                &BTreeMap::new(),
                receipts::CodexAppThread {
                    id: thread,
                    resumed: false,
                },
            )
            .expect("record Codex thread");
        }
        assert_eq!(other.fields().1, json!([]));
        assert_eq!(
            stored(&other.path())[KEY]["previous"]["provider_id"],
            "thread-1"
        );
    }

    #[test]
    fn a_changed_launch_path_or_cccc_location_is_named_apart_from_the_other_settings() {
        fn environment(launch: &[(&str, &str)]) -> BTreeMap<String, String> {
            launch
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect()
        }
        // The reasons a Claude start under `second` records after one under `first`.
        let claude = |first: &[(&str, &str)], second: &[(&str, &str)]| {
            let fixture = Fixture::new();
            for (launch, id) in [(first, FIRST), (second, SECOND)] {
                receipts::record_claude_managed_session(
                    &fixture.home,
                    &fixture.group,
                    "peer",
                    &fixture.cwd,
                    &["claude".to_owned()],
                    &environment(launch),
                    id,
                    false,
                )
                .expect("record Claude session");
            }
            stored(&fixture.path())[KEY]["reasons"].clone()
        };
        const SAVED: [(&str, &str); 3] =
            [("CCCC_CLI", "/a/cccc"), ("PATH", "/a"), ("SETTING", "a")];
        for (launch, reasons) in [
            (
                &[("CCCC_CLI", "/a/cccc"), ("PATH", "/b"), ("SETTING", "a")][..],
                json!(["launch_path"]),
            ),
            (
                &[("CCCC_CLI", "/b/cccc"), ("PATH", "/a"), ("SETTING", "a")][..],
                json!(["cccc_cli"]),
            ),
            (
                &[("CCCC_CLI", "/b/cccc"), ("PATH", "/b"), ("SETTING", "a")][..],
                json!(["launch_path", "cccc_cli"]),
            ),
            (
                &[("CCCC_CLI", "/a/cccc"), ("PATH", "/b"), ("SETTING", "b")][..],
                json!(["launch_path", "launch_identity"]),
            ),
            (
                &[("CCCC_CLI", "/a/cccc"), ("PATH", "/a"), ("SETTING", "b")][..],
                json!(["launch_identity"]),
            ),
            // An unset value is not an empty one.
            (
                &[("CCCC_CLI", "/a/cccc"), ("SETTING", "a")][..],
                json!(["launch_path"]),
            ),
        ] {
            assert_eq!(claude(&SAVED, launch), reasons, "{launch:?}");
        }
        assert_eq!(
            claude(&[("PATH", "")], &[]),
            json!(["launch_path"]),
            "an empty PATH, then none"
        );

        // A Claude launch file is neither of the two.
        let fixture = Fixture::new();
        for (settings, id) in [("{}", FIRST), (r#"{"changed":true}"#, SECOND)] {
            std::fs::write(fixture.cwd.join("settings.json"), settings).expect("launch file");
            receipts::record_claude_managed_session(
                &fixture.home,
                &fixture.group,
                "peer",
                &fixture.cwd,
                &["claude".to_owned(), "--settings=settings.json".to_owned()],
                &environment(&SAVED),
                id,
                false,
            )
            .expect("record Claude session");
        }
        assert_eq!(
            stored(&fixture.path())[KEY]["reasons"],
            json!(["launch_identity"])
        );

        // Pi and the native ACP runtimes keep the fingerprint inside `identity`.
        let fixture = Fixture::new();
        for (path, id) in [("/a", FIRST), ("/b", SECOND)] {
            let (identity, parts) = receipts::pi::launch(
                &fixture.cwd,
                &["pi".to_owned()],
                &environment(&[
                    ("HOME", "/probe/home"),
                    ("PATH", path),
                    ("PI_CODING_AGENT_DIR", "/probe/pi"),
                    ("USERPROFILE", "/probe/profile"),
                ]),
            )
            .expect("Pi launch identity");
            receipts::pi::record(
                &fixture.path(),
                &identity,
                parts.as_ref(),
                id,
                &fixture.cwd.join("session.jsonl"),
                true,
            )
            .expect("record Pi session");
        }
        assert_eq!(
            stored(&fixture.path())[KEY]["reasons"],
            json!(["launch_path"])
        );
        let acp = fixture.cwd.join("acp.json");
        for (cccc, id) in [("/a/cccc", "acp-1"), ("/b/cccc", "acp-2")] {
            let (identity, parts) = receipts::native_acp::launch(
                &fixture.cwd,
                &["kimi".to_owned()],
                &environment(&[("CCCC_CLI", cccc), ("PATH", "/a")]),
            );
            receipts::native_acp::record(&acp, &identity, &parts, id, true)
                .expect("record ACP session");
        }
        assert_eq!(stored(&acp)[KEY]["reasons"], json!(["cccc_cli"]));

        let noted = json!({"at":"2026-01-01T00:00:00Z","reasons":["launch_path","cccc_cli"]});
        assert_eq!(
            notice("g", "peer", &noted).data["message"],
            "peer started a new conversation. The one saved before it was not resumed. \
             Changed since it was saved: launch PATH, CCCC program location."
        );
    }

    #[test]
    fn a_fingerprint_is_reported_whole_unless_both_receipts_split_it_the_same_way() {
        fn parts(v: u64, path: &str) -> Value {
            json!({"v":v,"path":path,"cccc_cli":"c","rest":"r"})
        }
        let receipt = |fingerprint: &str, parts: Value| json!({"runtime":"claude","transport":"claude_agent_view","identity_fingerprint":fingerprint,"launch_parts":parts});
        let whole = vec!["launch_identity"];
        assert_eq!(
            reasons(&receipt("a", parts(1, "p1")), &receipt("b", parts(1, "p2"))),
            ["launch_path"]
        );
        for (case, saved, new) in [
            ("saved before the split", Value::Null, parts(1, "p2")),
            ("not recorded by the new one", parts(1, "p1"), Value::Null),
            // The same unknown format on both sides is still unknown.
            (
                "a format this build does not know",
                parts(2, "p1"),
                parts(2, "p2"),
            ),
            (
                "a part is missing",
                json!({"v":1,"path":"p1","cccc_cli":"c"}),
                parts(1, "p2"),
            ),
            (
                "a part is not a hash",
                json!({"v":1,"path":null,"cccc_cli":"c","rest":"r"}),
                parts(1, "p2"),
            ),
            (
                "no part accounts for the change",
                parts(1, "p1"),
                parts(1, "p1"),
            ),
        ] {
            assert_eq!(
                reasons(&receipt("a", saved), &receipt("b", new)),
                whole,
                "{case}"
            );
        }
        // Parts are read only to explain a fingerprint that changed.
        assert!(reasons(&receipt("a", parts(1, "p1")), &receipt("a", parts(1, "p2"))).is_empty());
    }

    #[test]
    fn an_antigravity_provider_home_or_distribution_change_is_a_launch_identity_change() {
        let fixture = Fixture::new();
        for (provider_home, id) in [("/gemini/a", FIRST), ("/gemini/b", SECOND)] {
            receipts::antigravity::record(
                &fixture.home,
                &fixture.group,
                "peer",
                &fixture.cwd,
                &["antigravity".to_owned()],
                &BTreeMap::from([("GEMINI_HOME".to_owned(), provider_home.to_owned())]),
                (id, false),
            )
            .expect("record Antigravity session");
        }
        let path = receipts::antigravity::receipt_path(&fixture.home, &fixture.group, "peer")
            .expect("Antigravity receipt path");
        assert_eq!(stored(&path)[KEY]["reasons"], json!(["launch_identity"]));

        // The identity also names the ACP distribution CCCC installed.
        let mut older = stored(&path);
        older["identity"]["version"] = json!("0.0.1");
        older["provider_session_id"] = json!(FIRST);
        older[KEY] = Value::Null;
        cccc_core::fs::write_json(&path, &older).expect("receipt of an older distribution");
        receipts::antigravity::record(
            &fixture.home,
            &fixture.group,
            "peer",
            &fixture.cwd,
            &["antigravity".to_owned()],
            &BTreeMap::from([("GEMINI_HOME".to_owned(), "/gemini/b".to_owned())]),
            (SECOND, false),
        )
        .expect("record after a distribution upgrade");
        assert_eq!(stored(&path)[KEY]["reasons"], json!(["launch_identity"]));
    }
}
