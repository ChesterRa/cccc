use super::*;
use anyhow::{Context, Result, anyhow, bail};
use cccc_contracts::AgentRuntimeSettings;
use std::collections::BTreeMap;

impl CodexVoiceSessions {
    pub(crate) async fn apply_analyst_settings(
        &self,
        home: &HomeLayout,
        settings: AgentRuntimeSettings,
        environment_set: BTreeMap<String, String>,
        environment_unset: Vec<String>,
        environment_clear: bool,
        discard_current_work: bool,
    ) -> Result<AnalystSettingsOutcome> {
        let settings = cccc_core::codex_voice_settings::normalize(settings)
            .context("validate Chet's Runtime settings")?;
        self.ensure_open()?;
        let sessions = self.clone();
        let home = home.clone();
        tokio::spawn(async move {
            sessions
                .apply_analyst_settings_transaction(
                    &home,
                    settings,
                    environment_set,
                    environment_unset,
                    environment_clear,
                    discard_current_work,
                )
                .await
        })
        .await
        .context("Chet settings worker failed")?
    }

    async fn apply_analyst_settings_transaction(
        &self,
        home: &HomeLayout,
        settings: AgentRuntimeSettings,
        environment_set: BTreeMap<String, String>,
        environment_unset: Vec<String>,
        environment_clear: bool,
        discard_current_work: bool,
    ) -> Result<AnalystSettingsOutcome> {
        // Serialize the read/compare/write/restart transaction. Reading the two files before this
        // lock would let concurrent administrators restore or overwrite one another's settings.
        let mut state = self.state.lock().await;
        self.ensure_open()?;
        let previous_settings = cccc_core::codex_voice_settings::load(home)?;
        let previous_environment = cccc_core::codex_voice_settings::private_environment(home)?;
        let resolved_previous_runtime = cccc_core::codex_voice_settings::resolve(
            home,
            &previous_settings,
            &previous_environment,
        )
        .ok();
        if settings.uses_profile()
            && (environment_clear || !environment_set.is_empty() || !environment_unset.is_empty())
        {
            bail!("Runtime Profile environment is managed in Settings > Runtime Profiles");
        }
        let environment_base = if environment_clear {
            BTreeMap::new()
        } else {
            previous_environment.clone()
        };
        let environment = cccc_core::codex_voice_settings::patched_private_environment(
            &environment_base,
            environment_set,
            &environment_unset,
        )?;
        let candidate_runtime =
            cccc_core::codex_voice_settings::resolve(home, &settings, &environment)
                .context("resolve Chet's Runtime configuration")?;
        let analyst_matches = state
            .analyst
            .as_ref()
            .is_none_or(|analyst| analyst.matches_launch(candidate_runtime.fingerprint()));
        if settings == previous_settings && environment == previous_environment && analyst_matches {
            return Ok(AnalystSettingsOutcome {
                analyst: state.analyst.as_ref().map(|analyst| analyst.info()),
                restarted: false,
                started_new_session: false,
                discarded_work: false,
            });
        }

        let previous = state.analyst.clone();
        let analyst_busy = if let Some(previous) = previous.as_ref() {
            previous.analyst.is_busy().await
        } else {
            false
        };
        validate_replacement(state.active.is_some(), analyst_busy, discard_current_work)?;
        if analyst_busy {
            tracing::info!(
                generation = %previous.as_ref().expect("busy Analyst exists").analyst.generation(),
                "discarding unfinished Voice Analyst work before applying confirmed settings"
            );
        }

        save_configuration(
            home,
            &settings,
            &environment,
            &previous_settings,
            &previous_environment,
        )?;
        let Some(previous) = state.analyst.clone() else {
            // While Chet is stopped, clear any prior start problem whenever the
            // saved configuration changes, except keep `claude_workspace_untrusted`
            // until Chet leaves Claude (unrelated Claude tweaks do not resolve trust).
            let mut slot = self
                .chet_start_problem
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let keep_untrusted = candidate_runtime.runtime == cccc_contracts::ActorRuntime::Claude
                && slot
                    .as_ref()
                    .and_then(|problem| problem.get("code"))
                    .and_then(serde_json::Value::as_str)
                    == Some("claude_workspace_untrusted");
            if !keep_untrusted {
                slot.take();
            }
            return Ok(AnalystSettingsOutcome {
                analyst: None,
                restarted: false,
                started_new_session: false,
                discarded_work: false,
            });
        };
        // A referenced Runtime Profile can be deleted or become invalid while a warm Analyst is
        // still alive. Preserve the exact effective launch snapshot so administrators can recover
        // to a valid Custom/Profile setting and rollback safely if the replacement fails.
        let previous_runtime =
            resolved_previous_runtime.unwrap_or_else(|| previous.launch_runtime());

        let materialized = (previous.analyst.tui_ready() || previous.analyst.structured_only())
            && previous.analyst.resumable();
        let identity_changed = cccc_core::codex_voice_settings::runtime_identity_changed(
            &previous_runtime,
            &candidate_runtime,
        );
        let resume_thread_id =
            (materialized && !identity_changed).then(|| previous.analyst.thread_id().to_owned());
        if previous.retire_for_replacement().await.is_err() {
            let pending = self.replacement_recovery_pending();
            if restore_configuration(home, &previous_settings, &previous_environment).is_err() {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice settings rollback failed; verify the saved configuration"
                ));
            }
            return Err(pending.into());
        }
        state.analyst = None;

        let workdir = cccc_core::codex_voice_settings::workdir(home)?;
        let warning = if identity_changed && materialized {
            "analyst_configuration_started_new_session".to_owned()
        } else {
            String::new()
        };
        let candidate = persistence::launch_exact(
            home,
            workdir.clone(),
            candidate_runtime,
            resume_thread_id.clone(),
            if resume_thread_id.is_some() {
                "ready"
            } else {
                "waiting"
            },
            warning,
        )
        .await;
        let replacement = match candidate {
            Ok(candidate) => Arc::new(candidate),
            Err(candidate_error) => {
                restore_configuration(home, &previous_settings, &previous_environment)?;
                let restored = persistence::launch_exact(
                    home,
                    workdir,
                    previous_runtime,
                    materialized.then(|| previous.analyst.thread_id().to_owned()),
                    if materialized { "ready" } else { "waiting" },
                    "analyst_configuration_rollback".into(),
                )
                .await
                .map(Arc::new)
                .map_err(|restore_error| {
                    anyhow!(
                        "Chet's new Runtime failed: {candidate_error}; the previous Runtime could not be restored: {restore_error}"
                    )
                })?;
                state.analyst = Some(Arc::clone(&restored));
                restored.start_monitor(home.clone(), Arc::clone(&self.chet_history_write_failed));
                if let Err(error) = persistence::persist_analyst(home, &restored, materialized) {
                    if restored.retire_for_replacement().await.is_err() {
                        return Err(self.replacement_recovery_pending().into());
                    }
                    state.analyst = None;
                    return Err(error.context("persist restored Chet session receipt"));
                }
                return Err(candidate_error.context(
                    "Chet settings were rejected; previous settings and Runtime were restored",
                ));
            }
        };
        state.analyst = Some(Arc::clone(&replacement));
        replacement.start_monitor(home.clone(), Arc::clone(&self.chet_history_write_failed));
        if let Err(error) =
            persistence::persist_analyst(home, &replacement, resume_thread_id.is_some())
        {
            if replacement.retire_for_replacement().await.is_err() {
                let pending = self.replacement_recovery_pending();
                if restore_configuration(home, &previous_settings, &previous_environment).is_err() {
                    cccc_runtime::diagnostic(format_args!(
                        "[cccc] Voice settings rollback failed; verify the saved configuration"
                    ));
                }
                return Err(pending.into());
            }
            state.analyst = None;
            restore_configuration(home, &previous_settings, &previous_environment)?;
            let restored = Arc::new(
                persistence::launch_exact(
                    home,
                    workdir,
                    previous_runtime,
                    materialized.then(|| previous.analyst.thread_id().to_owned()),
                    if materialized { "ready" } else { "waiting" },
                    "analyst_configuration_rollback".into(),
                )
                .await
                .context("restore the previous Chet session after receipt persistence failed")?,
            );
            state.analyst = Some(Arc::clone(&restored));
            restored.start_monitor(home.clone(), Arc::clone(&self.chet_history_write_failed));
            if let Err(restore_error) = persistence::persist_analyst(home, &restored, materialized)
            {
                if restored.retire_for_replacement().await.is_err() {
                    return Err(self.replacement_recovery_pending().into());
                }
                state.analyst = None;
                return Err(restore_error.context("persist restored Chet session receipt"));
            }
            return Err(error.context("persist replacement Chet session receipt"));
        }
        let info = replacement.info();
        state.analyst = Some(replacement);
        Ok(AnalystSettingsOutcome {
            analyst: Some(info),
            restarted: true,
            started_new_session: identity_changed && materialized,
            discarded_work: analyst_busy,
        })
    }
}

fn save_configuration(
    home: &HomeLayout,
    settings: &AgentRuntimeSettings,
    environment: &BTreeMap<String, String>,
    previous_settings: &AgentRuntimeSettings,
    previous_environment: &BTreeMap<String, String>,
) -> Result<()> {
    cccc_core::codex_voice_settings::save(home, settings)?;
    if let Err(error) =
        cccc_core::codex_voice_settings::replace_private_environment(home, environment)
    {
        cccc_core::codex_voice_settings::save(home, previous_settings).with_context(|| {
            format!("restore settings after private environment update failed: {error}")
        })?;
        cccc_core::codex_voice_settings::replace_private_environment(home, previous_environment)?;
        return Err(error.into());
    }
    Ok(())
}

fn restore_configuration(
    home: &HomeLayout,
    settings: &AgentRuntimeSettings,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    cccc_core::codex_voice_settings::save(home, settings)?;
    cccc_core::codex_voice_settings::replace_private_environment(home, environment)?;
    Ok(())
}

fn validate_replacement(
    call_active: bool,
    analyst_busy: bool,
    discard_current_work: bool,
) -> Result<()> {
    if call_active {
        bail!("Stop the active voice call before applying Chet settings");
    }
    if analyst_busy && !discard_current_work {
        bail!("Wait for or cancel Chet's current work before applying settings");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CodexVoiceSessions, validate_replacement};
    use cccc_contracts::{ActorRuntime, AgentRuntimeSettings};
    use cccc_core::HomeLayout;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn replacement_requires_separate_call_and_work_consent_boundaries() {
        assert!(validate_replacement(false, false, false).is_ok());
        assert!(validate_replacement(false, true, false).is_err());
        assert!(validate_replacement(false, true, true).is_ok());
        assert!(validate_replacement(true, false, true).is_err());
    }

    #[tokio::test]
    async fn changing_settings_while_stopped_clears_stale_start_problem() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let claude = AgentRuntimeSettings {
            runtime: ActorRuntime::Claude,
            ..Default::default()
        };
        cccc_core::codex_voice_settings::save(&home, &claude).expect("save initial claude");

        let sessions = CodexVoiceSessions::default();
        *sessions
            .chet_start_problem
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(json!({
            "code": "claude_workspace_untrusted",
            "workspace": "/tmp/fixture-untrusted",
        }));
        assert_eq!(
            sessions.chet_status().await["start_problem"]["code"],
            "claude_workspace_untrusted"
        );

        // Saving identical settings keeps the existing startup problem.
        sessions
            .apply_analyst_settings(&home, claude.clone(), BTreeMap::new(), vec![], false, false)
            .await
            .expect("unchanged settings");
        assert_eq!(
            sessions.chet_status().await["start_problem"]["code"],
            "claude_workspace_untrusted"
        );

        // Editing unrelated Claude fields (--model / generic env) keeps the workspace trust problem.
        let claude_tweaked = AgentRuntimeSettings {
            command: vec!["claude".into(), "--model".into(), "sonnet".into()],
            ..claude
        };
        sessions
            .apply_analyst_settings(
                &home,
                claude_tweaked,
                BTreeMap::from([("UNRELATED_FLAG".into(), "1".into())]),
                vec![],
                false,
                false,
            )
            .await
            .expect("unrelated claude settings update");
        assert_eq!(
            sessions.chet_status().await["start_problem"]["code"],
            "claude_workspace_untrusted"
        );

        // Switching away from Claude while Chet is stopped clears the stale problem immediately.
        let codex = AgentRuntimeSettings {
            runtime: ActorRuntime::Codex,
            ..Default::default()
        };
        sessions
            .apply_analyst_settings(&home, codex.clone(), BTreeMap::new(), vec![], false, false)
            .await
            .expect("switch runtime");
        assert!(sessions.chet_status().await["start_problem"].is_null());

        // For non-trust problems (e.g. start_failed / start_timeout / hermes_setup_required),
        // any meaningful settings change while stopped clears the warning immediately.
        *sessions
            .chet_start_problem
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(json!({"code": "start_failed"}));
        let codex_tweaked = AgentRuntimeSettings {
            command: vec!["codex".into(), "--model".into(), "o3".into()],
            ..codex
        };
        sessions
            .apply_analyst_settings(&home, codex_tweaked, BTreeMap::new(), vec![], false, false)
            .await
            .expect("update codex settings");
        assert!(sessions.chet_status().await["start_problem"].is_null());
    }
}
