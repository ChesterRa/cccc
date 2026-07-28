use crate::args::HookAction;
use anyhow::Result;
use cccc_core::HomeLayout;
use serde_json::Value;

pub fn run(home: &HomeLayout, action: HookAction) -> Result<()> {
    let launch_token = std::env::var("CCCC_HOOK_LAUNCH_TOKEN").unwrap_or_default();
    let group_id = std::env::var("CCCC_GROUP_ID").unwrap_or_default();
    let actor_id = std::env::var("CCCC_ACTOR_ID").unwrap_or_default();
    let payload: Value = serde_json::from_reader(std::io::stdin().lock())?;
    let runtime = match action {
        HookAction::CodexState => "codex",
        HookAction::ClaudeState => "claude",
    };
    cccc_core::codex_hook_state::record_runtime_with_observer(
        home,
        runtime,
        &group_id,
        &actor_id,
        &launch_token,
        &payload,
        |state, activity_authorized| {
            if activity_authorized {
                cccc_core::runtime_activity::record_hook_event(
                    home,
                    runtime,
                    &launch_token,
                    &payload,
                    state,
                )?;
            }
            Ok(())
        },
    )?;
    Ok(())
}
