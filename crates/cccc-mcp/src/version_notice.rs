//! A long-lived MCP server keeps the tools of the executable it started with,
//! while the daemon can restart on another CCCC version. After each
//! `tools/call`, the server compares its own version with the one the daemon
//! address file declares when read, which is not necessarily the process that
//! handled the call. A difference is reported once, and again whenever the
//! declared version changes to another one that differs.
//! Runtimes such as Claude Code and Codex show the model `structuredContent`
//! instead of the text items when it is present, so the notice goes into both.

use std::cmp::Ordering;

use cccc_core::HomeLayout;
use serde_json::{Value, json};

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const NOTICE_KEY: &str = "cccc_version_notice";

#[derive(Debug, Default)]
pub(crate) struct VersionNotice {
    /// The differing declared version already reported; cleared on a match.
    noticed: Option<String>,
}

impl VersionNotice {
    /// A missing, unreadable or malformed address file leaves the response
    /// unchanged; the next call checks again.
    pub(crate) async fn annotate(
        &mut self,
        home: &HomeLayout,
        runtime: Option<&str>,
        request: &Value,
        response: &mut Value,
    ) {
        if request.get("method").and_then(Value::as_str) != Some("tools/call") {
            return;
        }
        if let Some(declared) = declared_daemon_version(home).await {
            self.annotate_with(SERVER_VERSION, &declared, runtime, response);
        }
    }

    fn annotate_with(
        &mut self,
        server: &str,
        declared: &str,
        runtime: Option<&str>,
        response: &mut Value,
    ) {
        if declared == server {
            self.noticed = None;
            return;
        }
        if self.noticed.as_deref() == Some(declared) {
            return;
        }
        let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) else {
            return;
        };
        // Never replace a field of the tool's own result; a later result carries the notice.
        if result
            .get("structuredContent")
            .and_then(Value::as_object)
            .is_some_and(|structured| structured.contains_key(NOTICE_KEY))
        {
            return;
        }
        let text = notice_text(server, declared, runtime);
        let mut attached = false;
        if let Some(structured) = result
            .get_mut("structuredContent")
            .and_then(Value::as_object_mut)
        {
            structured.insert(NOTICE_KEY.into(), Value::String(text.clone()));
            attached = true;
        }
        if let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) {
            content.push(json!({"type":"text","text":text}));
            attached = true;
        }
        if attached {
            self.noticed = Some(declared.to_owned());
        }
    }
}

async fn declared_daemon_version(home: &HomeLayout) -> Option<String> {
    let raw = tokio::fs::read(cccc_client::address_path(home))
        .await
        .ok()?;
    let address: Value = serde_json::from_slice(&raw).ok()?;
    address
        .get("version")?
        .as_str()
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
}

fn notice_text(server: &str, daemon: &str, runtime: Option<&str>) -> String {
    let facts = format!(
        "CCCC version notice: this cccc MCP server is {server}, but the daemon address file declares {daemon}."
    );
    let step = restart_step(runtime);
    match compare_versions(server, daemon) {
        Some(Ordering::Less) => format!(
            "{facts} Its tools may be out of date until it restarts. Ask the user to restart it: {step}."
        ),
        Some(Ordering::Greater) => format!(
            "{facts} Its tools may not match the daemon until CCCC restarts on {server}. Ask the user to restart CCCC through its normal service controls."
        ),
        _ => format!(
            "{facts} Its tools may not match the daemon until both run the same version. Ask the user to restart whichever is older: CCCC through its normal service controls, or this MCP server ({step})."
        ),
    }
}

/// Mirrors "Upgrade an existing integration" in docs/guide/local-sessions.md.
fn restart_step(runtime: Option<&str>) -> &'static str {
    match runtime {
        Some("claude") => {
            "in Claude Code, open /mcp, select cccc and choose Reconnect, then run /reload-plugins"
        }
        Some("codex") => {
            "in Codex, let active work finish, then choose /daemon > Restart background server > Restart"
        }
        Some("pi") => "in Pi, run /reload",
        Some("opencode") => "in OpenCode, reload or reopen this session",
        _ => "use this application's MCP settings or restart the application",
    }
}

/// Orders plain `major.minor.patch` versions; anything else is left unordered.
fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    fn parse(version: &str) -> Option<[u64; 3]> {
        let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
        let parsed = [parts.next()??, parts.next()??, parts.next()??];
        parts.next().is_none().then_some(parsed)
    }
    match parse(left)?.cmp(&parse(right)?) {
        Ordering::Equal => None,
        order => Some(order),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> Value {
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"cccc_task","arguments":{}}})
    }

    fn result() -> Value {
        let payload = json!({"tasks":[{"id":"T1"}],"tasks_version":"tasksv:1"});
        json!({"jsonrpc":"2.0","id":7,"result":crate::router::tool_result(payload)})
    }

    fn home_with(address: Option<&str>) -> (tempfile::TempDir, HomeLayout) {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        if let Some(address) = address {
            let path = cccc_client::address_path(&home);
            std::fs::create_dir_all(path.parent().expect("daemon dir")).expect("daemon dir");
            std::fs::write(path, address).expect("address");
        }
        (temp, home)
    }

    fn address(version: &str) -> String {
        json!({"v":1,"transport":"unix","path":"/tmp/x.sock","pid":1,"version":version,"ts":"2026-11-04T00:00:00Z"}).to_string()
    }

    #[test]
    fn an_older_server_names_both_versions_and_the_runtime_step_once() {
        let mut notice = VersionNotice::default();
        let original = result();
        let mut first = original.clone();
        notice.annotate_with("0.4.47", "0.4.48", Some("claude"), &mut first);
        let structured = &first["result"]["structuredContent"];
        let text = structured[NOTICE_KEY].as_str().expect("structured notice");
        assert!(text.contains("this cccc MCP server is 0.4.47"), "{text}");
        assert!(
            text.contains("the daemon address file declares 0.4.48"),
            "{text}"
        );
        assert!(
            text.contains("open /mcp, select cccc and choose Reconnect"),
            "{text}"
        );
        // Existing data is untouched; only the key and a trailing text item are added.
        let mut without_key = structured.clone();
        without_key
            .as_object_mut()
            .expect("object")
            .remove(NOTICE_KEY);
        assert_eq!(without_key, original["result"]["structuredContent"]);
        let content = first["result"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], original["result"]["content"][0]);
        assert_eq!(content[1], json!({"type":"text","text":text}));

        let mut second = original.clone();
        notice.annotate_with("0.4.47", "0.4.48", Some("claude"), &mut second);
        assert_eq!(second, original);
    }

    #[test]
    fn a_newer_server_asks_for_a_cccc_restart_and_a_new_declared_version_notices_again() {
        let mut notice = VersionNotice::default();
        let mut older_daemon = result();
        notice.annotate_with("0.4.48", "0.4.47", Some("codex"), &mut older_daemon);
        let text = older_daemon["result"]["structuredContent"][NOTICE_KEY]
            .as_str()
            .expect("notice");
        assert!(text.contains("until CCCC restarts on 0.4.48"), "{text}");
        assert!(
            text.contains("restart CCCC through its normal service controls"),
            "{text}"
        );
        assert!(!text.contains("/daemon"), "{text}");

        let mut newer_daemon = result();
        notice.annotate_with("0.4.48", "0.4.49", Some("codex"), &mut newer_daemon);
        let text = newer_daemon["result"]["structuredContent"][NOTICE_KEY]
            .as_str()
            .expect("second notice");
        assert!(
            text.contains("/daemon > Restart background server > Restart"),
            "{text}"
        );
    }

    #[test]
    fn a_notice_repeats_only_after_the_declared_version_changes() {
        let mut notice = VersionNotice::default();
        let mut noticed = |declared: &str| {
            let mut response = result();
            notice.annotate_with("0.4.47", declared, None, &mut response);
            response["result"]["structuredContent"]
                .get(NOTICE_KEY)
                .is_some()
        };
        let observed: Vec<bool> = ["0.4.48", "0.4.48", "0.4.49", "0.4.48", "0.4.47", "0.4.47"]
            .into_iter()
            .chain(["0.4.48", "0.4.48"])
            .map(&mut noticed)
            .collect();
        // A match clears the state, so a later difference is a new one.
        assert_eq!(
            observed,
            [true, false, true, true, false, false, true, false]
        );
    }

    #[test]
    fn a_tool_result_that_already_uses_the_key_is_left_as_returned() {
        let mut notice = VersionNotice::default();
        let payload = json!({NOTICE_KEY: "tool data", "ok": true});
        let original = json!({"jsonrpc":"2.0","id":7,"result":crate::router::tool_result(payload)});
        let mut response = original.clone();
        notice.annotate_with("0.4.47", "0.4.48", None, &mut response);
        assert_eq!(response, original);
        let mut next = result();
        notice.annotate_with("0.4.47", "0.4.48", None, &mut next);
        assert!(next["result"]["structuredContent"][NOTICE_KEY].is_string());
    }

    #[test]
    fn matching_unordered_and_unattachable_cases() {
        let mut notice = VersionNotice::default();
        let mut same = result();
        notice.annotate_with("0.4.47", "0.4.47", None, &mut same);
        assert_eq!(same, result());

        // A JSON-RPC error has nowhere to carry the notice, so the next result does.
        let error = json!({"jsonrpc":"2.0","id":7,"error":{"code":-32601,"message":"x"}});
        let mut unattached = error.clone();
        notice.annotate_with("0.4.47", "0.5.0-rc1", None, &mut unattached);
        assert_eq!(unattached, error);
        let mut unordered = result();
        notice.annotate_with("0.4.47", "0.5.0-rc1", None, &mut unordered);
        let text = unordered["result"]["structuredContent"][NOTICE_KEY]
            .as_str()
            .expect("notice after an unattachable response");
        assert!(text.contains("restart whichever is older"), "{text}");
        assert!(
            text.ends_with("(use this application's MCP settings or restart the application)."),
            "{text}"
        );
    }

    #[tokio::test]
    async fn the_address_file_is_read_on_each_tool_call_and_bad_files_change_nothing() {
        let mut notice = VersionNotice::default();
        for bad in [
            None,
            Some("{not json"),
            Some(r#"{"v":1}"#),
            Some(r#"{"version":""}"#),
        ] {
            let (_temp, home) = home_with(bad);
            let mut response = result();
            notice.annotate(&home, None, &call(), &mut response).await;
            assert_eq!(response, result(), "{bad:?}");
        }

        let (_temp, home) = home_with(Some(&address(SERVER_VERSION)));
        let mut response = result();
        notice.annotate(&home, None, &call(), &mut response).await;
        assert_eq!(response, result());

        // A daemon restart on another version is seen by the next call, not a
        // cached address; tools/list and other methods are left alone.
        std::fs::write(cccc_client::address_path(&home), address("999.0.0")).expect("rewrite");
        let list = json!({"jsonrpc":"2.0","id":8,"method":"tools/list"});
        let mut listed = json!({"jsonrpc":"2.0","id":8,"result":{"tools":[]}});
        notice.annotate(&home, None, &list, &mut listed).await;
        assert_eq!(
            listed,
            json!({"jsonrpc":"2.0","id":8,"result":{"tools":[]}})
        );
        let mut response = result();
        notice.annotate(&home, None, &call(), &mut response).await;
        assert!(
            response["result"]["structuredContent"][NOTICE_KEY]
                .as_str()
                .is_some_and(|text| text.contains("declares 999.0.0"))
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare_versions("0.4.9", "0.4.10"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.0.0", "0.9.99"), Some(Ordering::Greater));
        assert_eq!(compare_versions("0.4.47", "0.4.47"), None);
        assert_eq!(compare_versions("0.4", "0.4.1"), None);
        assert_eq!(compare_versions("0.4.47-rc1", "0.4.48"), None);
    }
}
