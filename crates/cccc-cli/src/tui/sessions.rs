//! The native session list on the main screen and in `/setup`.

use super::daemon;
use cccc_client::DaemonClient;
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::time::Instant;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SessionList {
    /// The last list the daemon returned, in display order.
    pub(super) sources: Vec<LocalSessionSource>,
    pub(super) last_ok: Option<Instant>,
    /// Why the latest read failed; the next successful read clears it.
    pub(super) error: Option<String>,
}

impl SessionList {
    pub(super) fn record(&mut self, read: Result<Vec<LocalSessionSource>, String>, at: Instant) {
        match read {
            Ok(mut sources) => {
                sources.sort_by(display_order);
                self.sources = sources;
                self.last_ok = Some(at);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn receiving(&self) -> usize {
        self.sources
            .iter()
            .filter(|source| source.connected)
            .count()
    }
}

// Sessions that can receive come first; the rest of the order only keeps rows
// from trading places between refreshes.
fn display_order(left: &LocalSessionSource, right: &LocalSessionSource) -> Ordering {
    right
        .connected
        .cmp(&left.connected)
        .then_with(|| runtime_rank(left.runtime).cmp(&runtime_rank(right.runtime)))
        .then_with(|| left.cwd.cmp(&right.cwd))
        .then_with(|| left.title.cmp(&right.title))
        .then_with(|| left.source_id.cmp(&right.source_id))
}

fn runtime_rank(runtime: LocalSessionRuntime) -> u8 {
    match runtime {
        LocalSessionRuntime::Codex => 0,
        LocalSessionRuntime::Claude => 1,
        LocalSessionRuntime::Pi => 2,
        LocalSessionRuntime::Opencode => 3,
    }
}

pub(super) async fn read(client: &DaemonClient) -> Result<Vec<LocalSessionSource>, String> {
    let mut args = Map::new();
    args.insert("include_offline".into(), Value::Bool(true));
    let result = daemon::read(client, "local_session_list", args).await?;
    let sources = result
        .get("sources")
        .cloned()
        .ok_or_else(|| "the daemon answered without a session list".to_owned())?;
    serde_json::from_value(sources).map_err(|error| format!("unreadable session list ({error})"))
}

#[cfg(test)]
mod tests {
    use super::SessionList;
    use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
    use std::time::Instant;

    fn source(
        runtime: LocalSessionRuntime,
        id: &str,
        cwd: &str,
        connected: bool,
    ) -> LocalSessionSource {
        LocalSessionSource {
            source_id: runtime.source_id(id),
            runtime,
            native_session_id: id.into(),
            cwd: cwd.into(),
            title: String::new(),
            origin: None,
            connected,
        }
    }

    #[test]
    fn receiving_sessions_come_first_in_a_stable_order() {
        let mut list = SessionList::default();
        let now = Instant::now();
        list.record(
            Ok(vec![
                source(LocalSessionRuntime::Pi, "p", "/b", false),
                source(LocalSessionRuntime::Opencode, "o", "/a", true),
                source(LocalSessionRuntime::Codex, "c2", "/b", true),
                source(LocalSessionRuntime::Codex, "c1", "/a", true),
            ]),
            now,
        );
        let order: Vec<&str> = list
            .sources
            .iter()
            .map(|source| source.source_id.as_str())
            .collect();
        assert_eq!(order, ["codex:c1", "codex:c2", "opencode:o", "pi:p"]);
        assert_eq!(list.receiving(), 3);
        assert_eq!(list.last_ok, Some(now));
    }

    #[test]
    fn a_failed_read_keeps_the_last_list_and_its_time() {
        let mut list = SessionList::default();
        let first = Instant::now();
        list.record(
            Ok(vec![source(LocalSessionRuntime::Claude, "a", "/a", true)]),
            first,
        );
        list.record(Err("daemon unavailable".into()), Instant::now());
        assert_eq!(list.sources.len(), 1);
        assert_eq!(list.last_ok, Some(first));
        assert_eq!(list.error.as_deref(), Some("daemon unavailable"));

        list.record(Ok(Vec::new()), Instant::now());
        assert!(list.sources.is_empty());
        assert_eq!(list.error, None);
    }
}
