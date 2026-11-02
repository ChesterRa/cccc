use std::fmt::Write as _;
use std::time::Duration;

use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_contracts::{ActorRole, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout, active, actors};
use serde_json::Map;

pub async fn run(home: &HomeLayout, product_version: &str) -> Result<()> {
    let daemon_running = daemon_running(home).await;
    print!("{}", render(home, product_version, daemon_running)?);
    Ok(())
}

async fn daemon_running(home: &HomeLayout) -> bool {
    let response = DaemonClient::new(home.clone())
        .with_timeout(Duration::from_millis(750))
        .call(&DaemonRequest {
            v: 1,
            op: "ping".into(),
            args: Map::new(),
        })
        .await;
    response.is_ok_and(|response| response.ok)
}

fn render(home: &HomeLayout, product_version: &str, daemon_running: bool) -> Result<String> {
    let active_group_id = active::get(home)?.unwrap_or_default();
    let store = GroupStore::new(home.clone())?;
    let groups = store
        .list()?
        .into_iter()
        .filter_map(|meta| store.load(&meta.group_id).ok())
        .collect::<Vec<_>>();
    let available_runtimes = cccc_runtime::detect_runtimes()
        .into_iter()
        .filter(|runtime| runtime.available)
        .map(|runtime| runtime.name)
        .collect::<Vec<_>>();

    let mut output = String::new();
    writeln!(output, "CCCC Status")?;
    writeln!(output, "===========")?;
    writeln!(output, "Version:     {product_version}")?;
    writeln!(output, "Home:        {}", home.root().display())?;
    writeln!(
        output,
        "Daemon:      {}",
        if daemon_running { "running" } else { "stopped" }
    )?;
    writeln!(
        output,
        "Runtimes:    {}",
        if available_runtimes.is_empty() {
            "(none detected)".to_owned()
        } else {
            available_runtimes.join(", ")
        }
    )?;
    writeln!(output)?;

    if groups.is_empty() {
        writeln!(output, "Groups:      (none)")?;
        return Ok(output);
    }
    writeln!(output, "Groups:      {}", groups.len())?;
    for group in groups {
        let active_mark = if group.group_id == active_group_id {
            " *"
        } else {
            ""
        };
        let state = if group.running { "running" } else { "stopped" };
        writeln!(
            output,
            "  - {} ({}){} [{}]",
            group.title, group.group_id, active_mark, state
        )?;
        // Members as the Web and the terminal panel list them: internal helpers are not
        // shown, and the first member is the foreman whatever role was stored.
        for actor in actors::visible(&group) {
            let role = match actors::effective_role(&group, &actor.id) {
                Some(ActorRole::Foreman) => "foreman",
                _ => "peer",
            };
            let runtime = serde_json::to_value(actor.runtime)
                .ok()
                .and_then(|runtime| runtime.as_str().map(str::to_owned))
                .unwrap_or_else(|| "codex".into());
            writeln!(
                output,
                "      {} ({role}, {runtime}) [{}]",
                actor.id,
                if actor.enabled { "on" } else { "off" }
            )?;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_status_is_successful_without_retired_engine_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let output = render(&home, "0.4.34-rc2", false).expect("status");

        assert!(output.contains("Daemon:      stopped"));
        assert!(!output.contains("Selected:"));
        assert!(!output.contains("Python:"));
        assert!(!output.contains("Rust:"));
        assert!(output.contains("Groups:      (none)"));
    }

    #[test]
    fn status_lists_members_with_the_role_they_hold() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("demo", "").expect("group");
        // Stored roles are empty, as for members added today; an internal helper comes first.
        let mut helper = cccc_contracts::Actor::new("helper");
        helper.internal_kind = Some("fixture_helper".into());
        group.actors.push(helper);
        group.actors.push(cccc_contracts::Actor::new("lead"));
        group.actors.push(cccc_contracts::Actor::new("implementer"));
        store.save(&group).expect("save");

        let output = render(&home, "0.4.47", false).expect("status");

        assert!(output.contains("      lead (foreman, "), "{output}");
        assert!(output.contains("      implementer (peer, "), "{output}");
        assert!(!output.contains("helper"), "{output}");
    }
}
