use super::*;
use anyhow::{Context, Result, anyhow, bail};
use cccc_contracts::CodexVoiceAnalystSettings;
use std::collections::BTreeMap;

impl CodexVoiceSessions {
    pub(crate) async fn apply_analyst_settings(
        &self,
        home: &HomeLayout,
        settings: CodexVoiceAnalystSettings,
        environment_set: BTreeMap<String, String>,
        environment_unset: Vec<String>,
        environment_clear: bool,
    ) -> Result<AnalystSettingsOutcome> {
        let settings = cccc_core::codex_voice_settings::normalize(settings)
            .context("validate Voice Analyst launch settings")?;
        // Serialize the read/compare/write/restart transaction. Reading the two files before this
        // lock would let concurrent administrators restore or overwrite one another's settings.
        let mut state = self.state.lock().await;
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
                .context("resolve Voice Analyst runtime configuration")?;
        let analyst_matches = state
            .analyst
            .as_ref()
            .is_none_or(|analyst| analyst.matches_launch(candidate_runtime.fingerprint()));
        if settings == previous_settings && environment == previous_environment && analyst_matches {
            return Ok(AnalystSettingsOutcome {
                analyst: state.analyst.as_ref().map(|analyst| analyst.info()),
                restarted: false,
                started_new_session: false,
            });
        }

        if state.active.is_some() {
            bail!("Stop the active Codex Voice call before applying Analyst settings");
        }
        let previous = state.analyst.clone();
        if let Some(previous) = previous.as_ref()
            && previous.analyst.is_busy().await
        {
            bail!(
                "Wait for or cancel the current Voice Analyst investigation before applying settings"
            );
        }

        save_configuration(
            home,
            &settings,
            &environment,
            &previous_settings,
            &previous_environment,
        )?;
        let Some(previous) = state.analyst.take() else {
            return Ok(AnalystSettingsOutcome {
                analyst: None,
                restarted: false,
                started_new_session: false,
            });
        };
        // A referenced Runtime Profile can be deleted or become invalid while a warm Analyst is
        // still alive. Preserve the exact effective launch snapshot so administrators can recover
        // to a valid Custom/Profile setting and rollback safely if the replacement fails.
        let previous_runtime =
            resolved_previous_runtime.unwrap_or_else(|| previous.launch_runtime());

        let materialized = previous.analyst.tui_ready();
        let identity_changed = cccc_core::codex_voice_settings::identity_environment_changed(
            &previous_runtime.environment,
            &candidate_runtime.environment,
        );
        let resume_thread_id =
            (materialized && !identity_changed).then(|| previous.analyst.thread_id().to_owned());
        previous.stop_terminal();
        if let Err(error) = previous.analyst.shutdown().await {
            restore_configuration(home, &previous_settings, &previous_environment)?;
            state.analyst = Some(previous);
            return Err(error.context("stop previous Voice Analyst before applying settings"));
        }

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
                        "Voice Analyst candidate failed: {candidate_error}; previous runtime could not be restored: {restore_error}"
                    )
                })?;
                restored.start_monitor(home.clone(), self.ledger_events.clone());
                persistence::persist_analyst(home, &restored, materialized)?;
                state.analyst = Some(restored);
                return Err(candidate_error.context(
                    "Voice Analyst settings were rejected; previous settings and runtime were restored",
                ));
            }
        };
        replacement.start_monitor(home.clone(), self.ledger_events.clone());
        if let Err(error) =
            persistence::persist_analyst(home, &replacement, resume_thread_id.is_some())
        {
            replacement.stop_terminal();
            replacement.analyst.shutdown().await.ok();
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
                .context("restore previous Voice Analyst after receipt persistence failed")?,
            );
            restored.start_monitor(home.clone(), self.ledger_events.clone());
            persistence::persist_analyst(home, &restored, materialized)?;
            state.analyst = Some(restored);
            return Err(error.context("persist replacement Voice Analyst receipt"));
        }
        let info = replacement.info();
        state.analyst = Some(replacement);
        Ok(AnalystSettingsOutcome {
            analyst: Some(info),
            restarted: true,
            started_new_session: identity_changed && materialized,
        })
    }
}

fn save_configuration(
    home: &HomeLayout,
    settings: &CodexVoiceAnalystSettings,
    environment: &BTreeMap<String, String>,
    previous_settings: &CodexVoiceAnalystSettings,
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
    settings: &CodexVoiceAnalystSettings,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    cccc_core::codex_voice_settings::save(home, settings)?;
    cccc_core::codex_voice_settings::replace_private_environment(home, environment)?;
    Ok(())
}
