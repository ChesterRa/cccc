use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::space_credentials;
use serde_json::{Map, Value};
use tokio::time::Instant;

use super::{
    AuthFlowManager, BROWSER_HEIGHT, BROWSER_KEY, BROWSER_WIDTH, LOGIN_URL, PROVIDER, RunContext,
};

impl AuthFlowManager {
    pub(super) async fn run(self: Arc<Self>, mut context: RunContext) {
        let saved_raw = if context.force_reauth {
            None
        } else {
            space_credentials::resolve(&context.home, PROVIDER)
                .ok()
                .flatten()
        };
        if saved_raw.is_some() && validate(&context.client, None).await.is_ok() {
            self.finish(
                &context.session_id,
                "succeeded",
                "done",
                "Saved Google session is valid.",
                None,
            )
            .await;
            remove_profile(&context.profile).await;
            self.clear_active(&context.session_id).await;
            return;
        }
        if self
            .was_canceled(&context.session_id, &context.cancel)
            .await
        {
            remove_profile(&context.profile).await;
            self.clear_active(&context.session_id).await;
            return;
        }
        self.update(
            &context.session_id,
            "running",
            "preparing_browser",
            "Preparing the projected Google sign-in browser...",
            None,
        )
        .await;
        if let Err(error) = context.browsers.close(BROWSER_KEY).await {
            self.finish(
                &context.session_id,
                "failed",
                "failed",
                "Failed to reset the previous sign-in browser.",
                Some(error.to_string()),
            )
            .await;
            tracing::warn!(
                profile = %context.profile.display(),
                "keeping NotebookLM auth profile because the previous browser did not close cleanly"
            );
            self.clear_active(&context.session_id).await;
            return;
        }
        let storage_state = saved_raw
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        if let Err(error) = context
            .browsers
            .open_seeded_system(
                BROWSER_KEY,
                &context.profile,
                LOGIN_URL,
                BROWSER_WIDTH,
                BROWSER_HEIGHT,
                storage_state.as_ref(),
            )
            .await
        {
            self.finish(
                &context.session_id,
                "failed",
                "failed",
                "Failed to prepare the sign-in browser.",
                Some(error.to_string()),
            )
            .await;
            remove_profile(&context.profile).await;
            self.clear_active(&context.session_id).await;
            return;
        }
        self.update(
            &context.session_id,
            "running",
            "waiting_user_login",
            if storage_state.is_some() {
                "Restored saved cookies. Complete Google sign-in if prompted."
            } else {
                "Complete Google sign-in in the projected browser."
            },
            None,
        )
        .await;
        let deadline = Instant::now() + context.timeout;
        loop {
            tokio::select! {
                changed = context.cancel.changed() => {
                    if changed.is_err() || *context.cancel.borrow() { break; }
                }
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            if self
                .was_canceled(&context.session_id, &context.cancel)
                .await
            {
                break;
            }
            if Instant::now() >= deadline {
                self.finish(
                    &context.session_id,
                    "failed",
                    "failed",
                    "Google sign-in timed out.",
                    Some("authentication flow exceeded its timeout".into()),
                )
                .await;
                break;
            }
            if context.browsers.info(BROWSER_KEY).await["active"].as_bool() != Some(true) {
                self.finish(
                    &context.session_id,
                    "canceled",
                    "canceled",
                    "Google sign-in was canceled because the browser was closed.",
                    None,
                )
                .await;
                break;
            }
            if !context.browsers.page_available(BROWSER_KEY).await {
                self.finish(
                    &context.session_id,
                    "canceled",
                    "canceled",
                    "Google sign-in was canceled because the browser tab was closed.",
                    None,
                )
                .await;
                break;
            }
            if !context
                .browsers
                .notebooklm_auth_ready(BROWSER_KEY)
                .await
                .unwrap_or(false)
            {
                continue;
            }
            self.update(
                &context.session_id,
                "running",
                "verifying_session",
                "Verifying and saving the Google session...",
                None,
            )
            .await;
            let storage = match context.browsers.storage_state(BROWSER_KEY).await {
                Ok(storage) => storage,
                Err(error) => {
                    self.update(
                        &context.session_id,
                        "running",
                        "waiting_user_login",
                        "Sign-in detected, but browser cookies are not ready yet.",
                        Some(error.to_string()),
                    )
                    .await;
                    continue;
                }
            };
            let raw = match serde_json::to_string(&storage) {
                Ok(raw) => raw,
                Err(error) => {
                    self.finish(
                        &context.session_id,
                        "failed",
                        "failed",
                        "Failed to encode browser credentials.",
                        Some(error.to_string()),
                    )
                    .await;
                    break;
                }
            };
            match validate(&context.client, Some(&raw)).await {
                Ok(()) => {
                    if let Err(error) = space_credentials::update(&context.home, PROVIDER, &raw) {
                        self.finish(
                            &context.session_id,
                            "failed",
                            "failed",
                            "Failed to save browser credentials.",
                            Some(error.to_string()),
                        )
                        .await;
                        break;
                    }
                    if let Err(error) = validate(&context.client, None).await {
                        self.update(
                            &context.session_id,
                            "running",
                            "waiting_user_login",
                            "Session was saved, but provider activation is still pending.",
                            Some(error),
                        )
                        .await;
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                    self.finish(
                        &context.session_id,
                        "succeeded",
                        "done",
                        "Google account connected.",
                        None,
                    )
                    .await;
                    break;
                }
                Err(error) => {
                    self.update(
                        &context.session_id,
                        "running",
                        "waiting_user_login",
                        "Sign-in is incomplete. Keep the Gemini Notebook tab open.",
                        Some(error),
                    )
                    .await;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        close_browser_and_remove_profile(&context.browsers, &context.profile).await;
        self.clear_active(&context.session_id).await;
    }
}

pub(super) async fn close_browser_and_remove_profile(
    browsers: &crate::browser_surface::BrowserSurfaces,
    profile: &Path,
) {
    remove_profile_after_close(profile, browsers.close(BROWSER_KEY).await).await;
}

async fn remove_profile_after_close(profile: &Path, close_result: anyhow::Result<bool>) {
    match close_result {
        Ok(_) => remove_profile(profile).await,
        Err(error) => {
            tracing::warn!(
                %error,
                profile = %profile.display(),
                "keeping NotebookLM auth profile because its browser did not close cleanly"
            );
        }
    }
}

async fn validate(client: &DaemonClient, candidate: Option<&str>) -> Result<(), String> {
    let mut args = Map::from_iter([
        ("provider".into(), Value::String(PROVIDER.into())),
        ("by".into(), Value::String("user".into())),
    ]);
    if let Some(candidate) = candidate {
        args.insert("auth_json".into(), Value::String(candidate.into()));
    }
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "group_space_provider_health_check".into(),
            args,
        })
        .await
        .map_err(|error| error.to_string())?;
    if response.ok && response.result["healthy"].as_bool() == Some(true) {
        return Ok(());
    }
    Err(response
        .result
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("NotebookLM session validation failed")
        .to_owned())
}

pub(super) fn auth_timeout(seconds: u64) -> Duration {
    Duration::from_secs(seconds.clamp(60, 1_800))
}

pub(super) async fn remove_profile(profile: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(profile).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path=%profile.display(), %error, "failed to remove NotebookLM auth profile");
    }
}

#[cfg(test)]
mod tests {
    use super::remove_profile_after_close;

    #[tokio::test]
    async fn profile_is_removed_only_after_browser_close_succeeds() {
        let temp = tempfile::tempdir().expect("tempdir");
        let profile = temp.path().join("profile");
        tokio::fs::create_dir_all(&profile)
            .await
            .expect("create profile");

        remove_profile_after_close(&profile, Err(anyhow::anyhow!("browser still running"))).await;
        assert!(profile.exists(), "failed close must preserve the profile");

        remove_profile_after_close(&profile, Ok(true)).await;
        assert!(!profile.exists(), "successful close may remove the profile");
    }
}
