use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::dom::SetFileInputFilesParams;
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::BrowserSurfaces;
use super::navigation::goto_dom_content_loaded;

const COMPOSER_SELECTOR: &str = "[data-cccc-web-model-composer=\"cccc-web-model-composer\"]";
const COMPOSER_TIMEOUT: Duration = Duration::from_secs(30);
const PROMPT_STAGING_TIMEOUT: Duration = Duration::from_secs(3);
const SEND_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const SUBMISSION_EVIDENCE_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_INTERVAL: Duration = Duration::from_millis(150);
const SEND_STABILITY_INTERVAL: Duration = Duration::from_millis(300);
const RECEIPT_STABILITY_INTERVAL: Duration = Duration::from_secs(1);
const ATTACHMENT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const BOUND_CONVERSATION_ERROR_MARKER: &str = "chatgpt_bound_conversation_unavailable:";

pub(crate) enum PromptSubmissionOutcome {
    Verified(Value),
    Deferred(Value),
    Ambiguous(Value),
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ComposerCandidate {
    selector: String,
    descriptor: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct SendProbe {
    selector: String,
    descriptor: String,
    running: bool,
    stop_visible: bool,
    send_candidate_count: usize,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RequestSubmitResult {
    action: String,
    invoked: bool,
    unsafe_state: bool,
    error: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct SubmissionSnapshot {
    pub(super) url: String,
    echo_found: bool,
    pub(super) running: bool,
    pub(super) stop_visible: bool,
    composer_exact: bool,
    composer_contains_prompt: bool,
    pub(super) composer_chars: usize,
    pub(super) latest_turn_id: String,
    response_started: bool,
    pub(super) page_blocker: String,
    user_message_count: usize,
    send_enabled_count: usize,
}

enum SendReadiness {
    Ready(SendProbe),
    Deferred(SendProbe),
    Missing(SendProbe),
}

enum AttachmentOutcome {
    Ready,
    Deferred(Value),
}

struct SubmissionAttempt<'a> {
    prompt: &'a str,
    needles: &'a [String],
    input: &'a str,
    action: &'a str,
    baseline: &'a SubmissionSnapshot,
}

impl BrowserSurfaces {
    pub(crate) async fn inspect_staged_prompt(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
    ) -> Result<Value> {
        let page = self.page(key).await?;
        let current_url = page.url().await?.unwrap_or_default();
        let _ = wait_for_composer(&page).await?;
        let snapshot = inspect_submission(&page, prompt, &submission_needles(prompt)).await?;
        let recoverable = same_page(&current_url, target_url)
            && snapshot.user_message_count == 0
            && !snapshot.echo_found
            && !snapshot.running
            && !snapshot.stop_visible
            && snapshot.composer_exact;
        self.record_page_state(key, &page).await;
        Ok(json!({
            "recoverable":recoverable,
            "expected_target_url":target_url,
            "observed":snapshot
        }))
    }

    /// Resolve a ChatGPT optimistic echo with one bounded reload. This never dispatches again.
    /// It exists for the real case where the server accepted a message but the controlled tab
    /// remained on its client-side `request-*` placeholder.
    pub(crate) async fn refresh_optimistic_submission(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
        prior: &Value,
    ) -> Value {
        if prior["submission_evidence"] != "optimistic_echo_unconfirmed" {
            return prior.clone();
        }
        let action = prior["send_selector"].as_str().unwrap_or("");
        let input = prior["input_selector"].as_str().unwrap_or("");
        let baseline = serde_json::from_value::<SubmissionSnapshot>(prior["baseline"].clone())
            .unwrap_or_default();
        let observed = serde_json::from_value::<SubmissionSnapshot>(prior["observed"].clone())
            .unwrap_or_else(|_| baseline.clone());
        let finish = |kind: &str, error: Option<&str>| {
            let mut value = evidence(false, action, kind, input, &baseline, &observed);
            value["reconciled_by"] = json!("single_page_refresh");
            if let Some(error) = error {
                value["refresh_error"] = json!(error);
            }
            value
        };
        let page = match self.page(key).await {
            Ok(page) => page,
            Err(error) => {
                return finish(
                    "optimistic_echo_unconfirmed_after_refresh",
                    Some(&error.to_string()),
                );
            }
        };
        let current_url = page.url().await.ok().flatten().unwrap_or_default();
        if !same_page(&current_url, target_url) {
            return finish("submission_target_changed", None);
        }
        if let Err(error) = page.reload().await {
            return finish(
                "optimistic_echo_unconfirmed_after_refresh",
                Some(&error.to_string()),
            );
        }
        if let Err(error) = wait_for_composer(&page).await {
            return finish(
                "optimistic_echo_unconfirmed_after_refresh",
                Some(&error.to_string()),
            );
        }
        let needles = submission_needles(prompt);
        let outcome = self
            .verify_attempt(
                key,
                &page,
                SubmissionAttempt {
                    prompt,
                    needles: &needles,
                    input,
                    action,
                    baseline: &baseline,
                },
            )
            .await;
        let mut value = match outcome {
            PromptSubmissionOutcome::Verified(value)
            | PromptSubmissionOutcome::Deferred(value)
            | PromptSubmissionOutcome::Ambiguous(value) => value,
        };
        if value["submission_evidence"] == "optimistic_echo_unconfirmed" {
            value["submission_evidence"] = json!("optimistic_echo_unconfirmed_after_refresh");
        }
        value["reconciled_by"] = json!("single_page_refresh");
        value
    }

    #[cfg(test)]
    pub(crate) async fn submit_prompt_with_attachment(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
        attachment_path: Option<&Path>,
        delivery_id: &str,
    ) -> Result<PromptSubmissionOutcome> {
        self.submit_prompt_with_attachment_before_dispatch(
            key,
            target_url,
            prompt,
            attachment_path,
            delivery_id,
            || async { Ok(()) },
        )
        .await
    }

    // Err means no Send/Enter action was attempted. Once dispatch starts, all
    // uncertain outcomes are returned as Ambiguous, never as a retryable error.
    pub(crate) async fn submit_prompt_with_attachment_before_dispatch<F, Fut, Permit>(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
        attachment_path: Option<&Path>,
        delivery_id: &str,
        before_dispatch: F,
    ) -> Result<PromptSubmissionOutcome>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Permit>>,
    {
        if prompt.trim().is_empty() {
            bail!("browser prompt is empty");
        }
        let page = self.page(key).await?;
        let current_url = page.url().await?.unwrap_or_default();
        let current = inspect_submission(&page, prompt, &submission_needles(prompt)).await?;
        // Account-level refusal blocks navigation; an archived chat only blocks
        // itself. Do not cancel its archive or touch its messages.
        if !current.page_blocker.is_empty()
            && (current.page_blocker != "conversation_archived"
                || target_url.is_empty()
                || same_page(&current_url, target_url))
        {
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:page_blocked",
                &format!("not_sent_{}", current.page_blocker),
                "",
                &current,
                &current,
            )));
        }
        if !target_url.is_empty() && !same_page(&current_url, target_url) {
            if current.composer_chars > 0 {
                return Ok(PromptSubmissionOutcome::Deferred(evidence(
                    false,
                    "none:composer_occupied",
                    "not_sent_composer_occupied",
                    "",
                    &current,
                    &current,
                )));
            }
            if current.running || current.stop_visible {
                return Ok(PromptSubmissionOutcome::Deferred(evidence(
                    false,
                    "none:chat_busy",
                    "not_sent_chat_busy",
                    "",
                    &current,
                    &current,
                )));
            }
        }
        if has_chatgpt_conversation_route(target_url) {
            self.align_chatgpt_conversation_target(key, target_url, Duration::from_secs(5))
                .await?;
        } else if !target_url.is_empty() {
            let current = page.url().await?.unwrap_or_default();
            if !same_page(&current, target_url) {
                goto_dom_content_loaded(&page, target_url).await?;
            }
        }
        if sign_in_required(&page).await? {
            let observed = inspect_submission(&page, prompt, &submission_needles(prompt)).await?;
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:login_required",
                "not_sent_login_required",
                "",
                &observed,
                &observed,
            )));
        }
        dismiss_duplicate_upload_dialog(&page).await?;

        let needles = submission_needles(prompt);
        let existing = inspect_submission(&page, prompt, &needles).await?;
        if bound_target_changed(target_url, &existing.url) {
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:target_changed",
                "not_sent_target_changed",
                "",
                &existing,
                &existing,
            )));
        }
        if existing.echo_found {
            if provisional_submission(&existing) {
                return Ok(PromptSubmissionOutcome::Ambiguous(evidence(
                    false,
                    "existing:message_echo",
                    "optimistic_echo_unconfirmed",
                    "",
                    &existing,
                    &existing,
                )));
            }
            self.record_page_state(key, &page).await;
            return Ok(PromptSubmissionOutcome::Verified(with_receipt_needles(
                evidence(
                    true,
                    "existing:message_echo",
                    "message_echo",
                    "",
                    &existing,
                    &existing,
                ),
                &needles,
            )));
        }

        if !existing.page_blocker.is_empty() {
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:page_blocked",
                &format!("not_sent_{}", existing.page_blocker),
                "",
                &existing,
                &existing,
            )));
        }
        if existing.running || existing.stop_visible {
            self.record_page_state(key, &page).await;
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:chat_busy",
                "not_sent_chat_busy",
                "",
                &existing,
                &existing,
            )));
        }
        let composer = match wait_for_composer(&page).await {
            Ok(composer) => composer,
            Err(error) => {
                return Ok(PromptSubmissionOutcome::Deferred(json!({
                    "submitted":false, "submission_evidence":"not_sent_composer_unavailable",
                    "error":error.to_string(), "tab_url":page.url().await?.unwrap_or_default()
                })));
            }
        };
        let staged = inspect_submission(&page, prompt, &needles).await?;
        if bound_target_changed(target_url, &staged.url) {
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:target_changed",
                "not_sent_target_changed",
                "",
                &staged,
                &staged,
            )));
        }
        if staged.running
            || staged.stop_visible
            || (staged.composer_chars > 0 && !staged.composer_exact)
        {
            self.record_page_state(key, &page).await;
            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:composer_occupied",
                "not_sent_composer_occupied",
                &composer.descriptor,
                &staged,
                &staged,
            )));
        }
        if !staged.composer_exact {
            focus_and_select_composer(&page).await?;
            let ownership = json!({"install":true,"prompt":prompt,"delivery_id":delivery_id});
            page.evaluate(format!("({OBSERVE_STAGED_PROMPT_SCRIPT})({ownership})"))
                .await?;
            let inserted = page.execute(InsertTextParams::new(prompt)).await;
            let unobserved = page
                .evaluate(format!(
                    "({OBSERVE_STAGED_PROMPT_SCRIPT})({{install:false}})"
                ))
                .await;
            inserted.context("insert prompt into visible browser composer")?;
            unobserved?;
        }
        let mut baseline = wait_for_prompt_staged(&page, prompt, &needles).await?;
        if let Some(path) = attachment_path {
            let attachment = attach_compatibility_image(&page, path, delivery_id).await?;
            if let AttachmentOutcome::Deferred(attachment) = attachment {
                let outcome = self
                    .classify_deferred_submission(
                        key,
                        &page,
                        SubmissionAttempt {
                            prompt,
                            needles: &needles,
                            input: &composer.descriptor,
                            action: "attachment:file_input_dispatch",
                            baseline: &baseline,
                        },
                        "attachment_not_ready",
                    )
                    .await;
                return Ok(with_attachment_evidence(outcome, attachment));
            }
            baseline = inspect_submission(&page, prompt, &needles).await?;
            if !baseline.composer_exact {
                bail!("browser prompt changed while attaching the compatibility image");
            }
        }

        let readiness = wait_for_send_control(&page).await?;
        match readiness {
            SendReadiness::Deferred(probe) => Ok(self
                .classify_deferred_submission(
                    key,
                    &page,
                    SubmissionAttempt {
                        prompt,
                        needles: &needles,
                        input: &composer.descriptor,
                        action: &probe.descriptor,
                        baseline: &baseline,
                    },
                    "send_control_deferred",
                )
                .await),
            SendReadiness::Ready(probe) | SendReadiness::Missing(probe) => {
                let permit = match before_dispatch().await {
                    Ok(permit) => permit,
                    Err(error) => {
                        self.clear_staged_prompt(key, prompt, delivery_id).await?;
                        return Err(error);
                    }
                };
                let request_submit = request_submit(&page, target_url).await;
                match request_submit {
                    Ok(result) if result.unsafe_state => {
                        drop(permit);
                        Ok(self
                            .classify_deferred_submission(
                                key,
                                &page,
                                SubmissionAttempt {
                                    prompt,
                                    needles: &needles,
                                    input: &composer.descriptor,
                                    action: &result.action,
                                    baseline: &baseline,
                                },
                                "send_control_deferred",
                            )
                            .await)
                    }
                    Ok(result) if result.invoked => {
                        drop(permit);
                        let action = if result.error.is_empty() {
                            result.action
                        } else {
                            format!("{}:dispatch_unknown", result.action)
                        };
                        Ok(self
                            .verify_attempt(
                                key,
                                &page,
                                SubmissionAttempt {
                                    prompt,
                                    needles: &needles,
                                    input: &composer.descriptor,
                                    action: &action,
                                    baseline: &baseline,
                                },
                            )
                            .await)
                    }
                    Ok(_) if probe.running || probe.stop_visible => {
                        drop(permit);
                        Ok(self
                            .classify_deferred_submission(
                                key,
                                &page,
                                SubmissionAttempt {
                                    prompt,
                                    needles: &needles,
                                    input: &composer.descriptor,
                                    action: &probe.descriptor,
                                    baseline: &baseline,
                                },
                                "send_control_deferred",
                            )
                            .await)
                    }
                    Ok(_) => {
                        let current = page.url().await.ok().flatten().unwrap_or_default();
                        if bound_target_changed(target_url, &current) {
                            return Ok(PromptSubmissionOutcome::Deferred(evidence(
                                false,
                                "none:target_changed",
                                "not_sent_target_changed",
                                &composer.descriptor,
                                &baseline,
                                &baseline,
                            )));
                        }
                        let action = "keyboard:Enter";
                        let press = match page.find_element(COMPOSER_SELECTOR).await {
                            Ok(input) => input.press_key("Enter").await.map(|_| ()),
                            Err(error) => Err(error),
                        };
                        drop(permit);
                        let action = if press.is_ok() {
                            action.to_owned()
                        } else {
                            format!("{action}:dispatch_unknown")
                        };
                        Ok(self
                            .verify_attempt(
                                key,
                                &page,
                                SubmissionAttempt {
                                    prompt,
                                    needles: &needles,
                                    input: &composer.descriptor,
                                    action: &action,
                                    baseline: &baseline,
                                },
                            )
                            .await)
                    }
                    Err(_) => {
                        drop(permit);
                        Ok(self
                            .verify_attempt(
                                key,
                                &page,
                                SubmissionAttempt {
                                    prompt,
                                    needles: &needles,
                                    input: &composer.descriptor,
                                    action: "form.requestSubmit:dispatch_unknown",
                                    baseline: &baseline,
                                },
                            )
                            .await)
                    }
                }
            }
        }
    }

    pub(crate) async fn clear_staged_prompt(
        &self,
        key: &str,
        prompt: &str,
        delivery_id: &str,
    ) -> Result<()> {
        let page = self.page(key).await?;
        let payload = json!({"prompt":prompt,"delivery_id":delivery_id});
        let cleared = page
            .evaluate(format!("({CLEAR_STAGED_PROMPT_SCRIPT})({payload})"))
            .await?
            .into_value::<bool>()?;
        if !cleared {
            bail!("could not clear the cancelled browser prompt");
        }
        Ok(())
    }

    async fn classify_deferred_submission(
        &self,
        key: &str,
        page: &Page,
        attempt: SubmissionAttempt<'_>,
        deferred_evidence: &str,
    ) -> PromptSubmissionOutcome {
        let observed = match inspect_submission(page, attempt.prompt, attempt.needles).await {
            Ok(observed) => observed,
            Err(error) => {
                tracing::debug!(%error, "failed to recheck a deferred browser submission");
                self.record_page_state(key, page).await;
                return PromptSubmissionOutcome::Ambiguous(evidence(
                    false,
                    attempt.action,
                    "deferred_state_unverifiable",
                    attempt.input,
                    attempt.baseline,
                    attempt.baseline,
                ));
            }
        };
        self.record_page_state(key, page).await;
        if bound_target_changed(&attempt.baseline.url, &observed.url) {
            return PromptSubmissionOutcome::Deferred(evidence(
                false,
                "none:target_changed",
                "not_sent_target_changed",
                attempt.input,
                attempt.baseline,
                &observed,
            ));
        }
        if observed.echo_found && !provisional_submission(&observed) {
            return PromptSubmissionOutcome::Verified(with_receipt_needles(
                evidence(
                    true,
                    attempt.action,
                    "message_echo",
                    attempt.input,
                    attempt.baseline,
                    &observed,
                ),
                attempt.needles,
            ));
        }
        if let Some(submission_evidence) = verified_submission_evidence(attempt.baseline, &observed)
        {
            return PromptSubmissionOutcome::Verified(with_receipt_needles(
                evidence(
                    true,
                    attempt.action,
                    submission_evidence,
                    attempt.input,
                    attempt.baseline,
                    &observed,
                ),
                attempt.needles,
            ));
        }
        if let Some(submission_evidence) = weak_submission_evidence(attempt.baseline, &observed) {
            return PromptSubmissionOutcome::Ambiguous(evidence(
                false,
                attempt.action,
                submission_evidence,
                attempt.input,
                attempt.baseline,
                &observed,
            ));
        }
        PromptSubmissionOutcome::Deferred(evidence(
            false,
            attempt.action,
            deferred_evidence,
            attempt.input,
            attempt.baseline,
            &observed,
        ))
    }

    async fn verify_attempt(
        &self,
        key: &str,
        page: &Page,
        attempt: SubmissionAttempt<'_>,
    ) -> PromptSubmissionOutcome {
        let SubmissionAttempt {
            prompt,
            needles,
            input,
            action,
            baseline,
        } = attempt;
        let deadline = Instant::now() + SUBMISSION_EVIDENCE_TIMEOUT;
        let mut latest = baseline.clone();
        let mut weak_evidence = None;
        loop {
            match inspect_submission(page, prompt, needles).await {
                Ok(snapshot) => {
                    if bound_target_changed(&baseline.url, &snapshot.url) {
                        return PromptSubmissionOutcome::Ambiguous(evidence(
                            false,
                            action,
                            "submission_target_changed",
                            input,
                            baseline,
                            &snapshot,
                        ));
                    }
                    if let Some(submission_evidence) = verified_submission_kind(baseline, &snapshot)
                    {
                        self.record_page_state(key, page).await;
                        return PromptSubmissionOutcome::Verified(with_receipt_needles(
                            evidence(
                                true,
                                action,
                                submission_evidence,
                                input,
                                baseline,
                                &snapshot,
                            ),
                            needles,
                        ));
                    }
                    weak_evidence = weak_submission_evidence(baseline, &snapshot)
                        .map(str::to_owned)
                        .or(weak_evidence);
                    latest = snapshot;
                }
                Err(error) => {
                    tracing::debug!(%error, "failed to inspect browser submission evidence");
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        self.record_page_state(key, page).await;
        PromptSubmissionOutcome::Ambiguous(evidence(
            false,
            action,
            weak_evidence
                .as_deref()
                .unwrap_or("submission_verification_ambiguous"),
            input,
            baseline,
            &latest,
        ))
    }

    pub(super) async fn page(&self, key: &str) -> Result<Page> {
        self.sessions
            .lock()
            .await
            .get(key)
            .map(|session| session.page.clone())
            .context("browser surface is not active")
    }

    pub(crate) async fn navigate_to_url(&self, key: &str, target_url: &str) -> Result<String> {
        let page = self.page(key).await?;
        let current = page.url().await?.unwrap_or_default();
        if !same_page(&current, target_url) {
            goto_dom_content_loaded(&page, target_url).await?;
        }
        let observed = page.url().await?.unwrap_or_default();
        self.record_page_state(key, &page).await;
        Ok(observed)
    }

    pub(crate) async fn align_chatgpt_conversation_target(
        &self,
        key: &str,
        target_url: &str,
        timeout: Duration,
    ) -> Result<String> {
        let expected = normalized_chatgpt_conversation_url(target_url).ok_or_else(|| {
            anyhow::anyhow!(
                "{BOUND_CONVERSATION_ERROR_MARKER} saved ChatGPT conversation URL is provisional or invalid"
            )
        })?;
        let page = self.page(key).await?;
        let current = page.url().await?.unwrap_or_default();
        if conversation_target_matches(&expected, &current) {
            self.record_page_state(key, &page).await;
            return Ok(expected);
        }
        goto_dom_content_loaded(&page, &expected).await?;
        let deadline = Instant::now() + timeout;
        let mut consecutive = 0_u8;
        loop {
            let observed = page.url().await?.unwrap_or_default();
            if conversation_target_matches(&expected, &observed) {
                consecutive += 1;
                if consecutive >= 2 {
                    self.record_page_state(key, &page).await;
                    return Ok(expected);
                }
            } else {
                consecutive = 0;
            }
            if Instant::now() >= deadline {
                self.record_page_state(key, &page).await;
                bail!("{BOUND_CONVERSATION_ERROR_MARKER} expected={expected} observed={observed}");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub(crate) async fn wait_for_conversation_url(
        &self,
        key: &str,
        target_url: &str,
        timeout: Duration,
    ) -> Result<Option<String>> {
        let page = self.page(key).await?;
        let deadline = Instant::now() + timeout;
        let mut candidate = None::<String>;
        let mut consecutive = 0_u8;
        loop {
            let current = page.url().await?.unwrap_or_default();
            if let Some(conversation_url) = conversation_url_for_target(target_url, &current) {
                if timeout.is_zero() {
                    self.record_page_state(key, &page).await;
                    return Ok(Some(conversation_url));
                }
                if candidate.as_deref() == Some(conversation_url.as_str()) {
                    consecutive += 1;
                } else {
                    candidate = Some(conversation_url);
                    consecutive = 1;
                }
                if consecutive >= 2 {
                    self.record_page_state(key, &page).await;
                    return Ok(candidate);
                }
            } else {
                candidate = None;
                consecutive = 0;
            }
            if Instant::now() >= deadline {
                self.record_page_state(key, &page).await;
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub(crate) async fn relay_surface_idle(&self, key: &str) -> Result<bool> {
        Ok(self.relay_surface_deferral(key).await?.is_none())
    }

    pub(crate) async fn relay_surface_deferral(&self, key: &str) -> Result<Option<Value>> {
        let page = self.page(key).await?;
        let snapshot = inspect_submission(&page, "__cccc_relay_busy_probe__", &[]).await?;
        let blocker = format!("not_sent_{}", snapshot.page_blocker);
        let reason = if !snapshot.page_blocker.is_empty() {
            blocker.as_str()
        } else if sign_in_required(&page).await? {
            "not_sent_login_required"
        } else if snapshot.running || snapshot.stop_visible {
            "not_sent_chat_busy"
        } else if snapshot.composer_chars > 0 {
            "not_sent_composer_occupied"
        } else if !composer_present(&page).await? {
            "not_sent_composer_unavailable"
        } else {
            return Ok(None);
        };
        let mut evidence = serde_json::to_value(snapshot).context("encode relay deferral")?;
        evidence["submitted"] = json!(false);
        evidence["submission_evidence"] = json!(reason);
        Ok(Some(evidence))
    }

    pub(crate) async fn relay_receipt_stable_before_close(
        &self,
        key: &str,
        evidence: &Value,
    ) -> Result<bool> {
        let needles = evidence["receipt_needles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if needles.is_empty() {
            return Ok(false);
        }
        let expected_url = evidence["tab_url"].as_str().unwrap_or("");
        let page = self.page(key).await?;
        let deadline = Instant::now() + RECEIPT_STABILITY_INTERVAL;
        loop {
            let snapshot = inspect_submission(&page, "", &needles).await?;
            let stable = snapshot.echo_found
                && !provisional_submission(&snapshot)
                && snapshot.page_blocker.is_empty()
                && (expected_url.is_empty() || same_page(expected_url, &snapshot.url));
            if !stable {
                self.record_page_state(key, &page).await;
                return Ok(false);
            }
            if Instant::now() >= deadline {
                self.record_page_state(key, &page).await;
                return Ok(true);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub(crate) async fn relay_target_deferral(
        &self,
        key: &str,
        target_url: &str,
    ) -> Result<Option<Value>> {
        Ok(self.relay_surface_deferral(key).await?.filter(|blocked| {
            !matches!(
                blocked["submission_evidence"].as_str(),
                Some("not_sent_composer_unavailable" | "not_sent_conversation_archived")
            ) || target_url.is_empty()
                || same_page(blocked["url"].as_str().unwrap_or(""), target_url)
        }))
    }

    pub(crate) async fn prompt_readiness(&self, key: &str) -> Result<Value> {
        let page = self.page(key).await?;
        let url = page.url().await?.unwrap_or_default();
        let candidate = page
            .evaluate(format!("({SELECT_COMPOSER_SCRIPT})()"))
            .await
            .context("inspect visible browser composer")?
            .into_value::<ComposerCandidate>()
            .context("decode visible browser composer")?;
        let login_required = sign_in_required(&page).await?;
        let page_blocker = inspect_submission(&page, "", &[]).await?.page_blocker;
        let ready = !candidate.selector.is_empty() && !login_required && page_blocker.is_empty();
        let readiness = json!({
            "ready":ready,
            "login_required":login_required,
            "tab_url":url,
            "input_selector":candidate.descriptor,
            "page_blocker":page_blocker,
            "checked_at":cccc_contracts::utc_now(),
            "message":if !page_blocker.is_empty() {
                "The conversation is archived or browser access is restricted. The report is retained; automatic page recovery is stopped."
            } else if login_required {
                "Sign in to ChatGPT in this browser before expecting tool access or saved-conversation delivery."
            } else if ready {
                "Browser model composer is ready; account tool access still requires a real call."
            } else {
                "Browser model composer is not ready yet."
            }
        });
        self.record_prompt_readiness(key, &page, &readiness).await;
        Ok(readiness)
    }

    async fn record_prompt_readiness(&self, key: &str, page: &Page, readiness: &Value) {
        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get_mut(key) else {
            return;
        };
        if session.page.target_id() != page.target_id() {
            return;
        }
        if let Some(url) = readiness["tab_url"].as_str() {
            session.url = url.to_owned();
        }
        if let Some(metadata) = session.metadata.as_object_mut() {
            metadata.insert("prompt_readiness".into(), readiness.clone());
        }
        session.updated_at = cccc_contracts::utc_now();
    }

    async fn record_page_state(&self, key: &str, page: &Page) {
        let url = page.url().await.ok().flatten();
        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get_mut(key) else {
            return;
        };
        if session.page.target_id() != page.target_id() {
            return;
        }
        if let Some(url) = url {
            session.url = url;
        }
        session.updated_at = cccc_contracts::utc_now();
    }
}

async fn composer_present(page: &Page) -> Result<bool> {
    let input: Value = page
        .evaluate(format!("({SELECT_COMPOSER_SCRIPT})()"))
        .await?
        .into_value()?;
    Ok(input["selector"].as_str().is_some_and(|s| !s.is_empty()))
}

async fn dismiss_duplicate_upload_dialog(page: &Page) -> Result<()> {
    let action = page
        .evaluate(format!("({DISMISS_DUPLICATE_UPLOAD_DIALOG_SCRIPT})()"))
        .await
        .context("inspect duplicate compatibility image dialog")?
        .into_value::<String>()
        .context("decode duplicate compatibility image dialog state")?;
    match action.as_str() {
        "none" => return Ok(()),
        "dismissed" => {}
        _ => bail!("ChatGPT duplicate compatibility image dialog could not be dismissed"),
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let visible = page
            .evaluate(format!("({DUPLICATE_UPLOAD_DIALOG_VISIBLE_SCRIPT})()"))
            .await
            .context("wait for duplicate compatibility image dialog to close")?
            .into_value::<bool>()
            .context("decode duplicate compatibility image dialog visibility")?;
        if !visible {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("ChatGPT duplicate compatibility image dialog remained open after dismissal");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn attachment_status(page: &Page, delivery_id: &str, filename: &str) -> Result<Value> {
    let payload = serde_json::to_string(&json!({
        "delivery_id":delivery_id,
        "filename":filename
    }))?;
    page.evaluate(format!("({ATTACHMENT_STATUS_SCRIPT})({payload})"))
        .await
        .context("inspect compatibility image attachment")?
        .into_value::<Value>()
        .context("decode compatibility image attachment state")
}

async fn attach_compatibility_image(
    page: &Page,
    path: &Path,
    delivery_id: &str,
) -> Result<AttachmentOutcome> {
    if delivery_id.trim().is_empty() {
        bail!("compatibility image delivery_id is required");
    }
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve compatibility image {}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .context("compatibility image filename is not valid UTF-8")?;
    let delivery_json = serde_json::to_string(delivery_id)?;
    let existing = attachment_status(page, delivery_id, filename).await?;
    if existing["ready"].as_bool().unwrap_or(false) {
        let _ = page
            .evaluate(format!(
                "document.documentElement.dataset.ccccWebModelAttachmentDelivery = {delivery_json}"
            ))
            .await;
        return Ok(AttachmentOutcome::Ready);
    }
    let input = page
        .find_element("#upload-photos, input[type=file][accept*='image']")
        .await
        .context("find ChatGPT image upload input")?;
    let dispatched = page
        .execute(
            SetFileInputFilesParams::builder()
                .file(path.to_string_lossy().into_owned())
                .backend_node_id(input.backend_node_id)
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await;
    if let Err(error) = dispatched {
        return Ok(AttachmentOutcome::Deferred(json!({
            "delivery_id":delivery_id,
            "filename":filename,
            "dispatched":false,
            "ready":false,
            "reason":"file_input_dispatch_unknown",
            "error":error.to_string()
        })));
    }
    let _ = page
        .evaluate(format!(
            "document.documentElement.dataset.ccccWebModelAttachmentDispatched = {delivery_json}"
        ))
        .await;
    let deadline = Instant::now() + ATTACHMENT_TIMEOUT;
    loop {
        let latest = match attachment_status(page, delivery_id, filename).await {
            Ok(mut status) => {
                if let Some(object) = status.as_object_mut() {
                    object.insert("delivery_id".into(), json!(delivery_id));
                    object.insert("filename".into(), json!(filename));
                    object.insert("dispatched".into(), json!(true));
                }
                if status["ready"].as_bool().unwrap_or(false) {
                    let _ = page
                        .evaluate(format!(
                            "document.documentElement.dataset.ccccWebModelAttachmentDelivery = {delivery_json}"
                        ))
                        .await;
                    return Ok(AttachmentOutcome::Ready);
                }
                status
            }
            Err(error) => json!({
                "delivery_id":delivery_id,
                "filename":filename,
                "dispatched":true,
                "ready":false,
                "reason":"attachment_readiness_unknown",
                "error":error.to_string()
            }),
        };
        if Instant::now() >= deadline {
            return Ok(AttachmentOutcome::Deferred(latest));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn bound_target_changed(expected: &str, observed: &str) -> bool {
    !expected.is_empty()
        && (has_chatgpt_conversation_route(expected) || !is_chatgpt_url(expected))
        && !same_page(expected, observed)
}

fn same_page(left: &str, right: &str) -> bool {
    let Ok(mut left) = reqwest::Url::parse(left) else {
        return false;
    };
    let Ok(mut right) = reqwest::Url::parse(right) else {
        return false;
    };
    left.set_query(None);
    left.set_fragment(None);
    right.set_query(None);
    right.set_fragment(None);
    left == right
}

async fn wait_for_composer(page: &Page) -> Result<ComposerCandidate> {
    let deadline = Instant::now() + COMPOSER_TIMEOUT;
    let mut last_error = None;
    loop {
        match page.evaluate(format!("({SELECT_COMPOSER_SCRIPT})()")).await {
            Ok(value) => match value.into_value::<ComposerCandidate>() {
                Ok(candidate) if !candidate.selector.is_empty() => return Ok(candidate),
                Ok(_) => {}
                Err(error) => last_error = Some(error.to_string()),
            },
            Err(error) => last_error = Some(error.to_string()),
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    bail!(
        "visible browser composer not found{}",
        last_error.map_or_else(String::new, |error| format!("; last_error={error}"))
    )
}

async fn focus_and_select_composer(page: &Page) -> Result<()> {
    let focused = page
        .evaluate(format!("({FOCUS_AND_SELECT_SCRIPT})()"))
        .await
        .context("focus visible browser composer")?
        .into_value::<bool>()
        .context("decode browser composer focus result")?;
    if !focused {
        bail!("visible browser composer disappeared before prompt insertion");
    }
    Ok(())
}

async fn wait_for_prompt_staged(
    page: &Page,
    prompt: &str,
    needles: &[String],
) -> Result<SubmissionSnapshot> {
    let deadline = Instant::now() + PROMPT_STAGING_TIMEOUT;
    loop {
        let snapshot = inspect_submission(page, prompt, needles).await?;
        if snapshot.composer_exact {
            return Ok(snapshot);
        }
        if Instant::now() >= deadline {
            bail!(
                "browser prompt insertion did not stick; composer_chars={}",
                snapshot.composer_chars
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_send_control(page: &Page) -> Result<SendReadiness> {
    let deadline = Instant::now() + SEND_CONTROL_TIMEOUT;
    let mut stable_descriptor = String::new();
    let mut stable_since = Instant::now();
    let mut stop_only_since = None;
    loop {
        let probe = page
            .evaluate(format!(
                "({SELECT_SEND_CONTROL_SCRIPT})({GENERATION_STOP_CONTROL_SCRIPT})"
            ))
            .await
            .context("inspect browser composer send control")?
            .into_value::<SendProbe>()
            .context("decode browser composer send control")?;
        if !probe.selector.is_empty() {
            if probe.descriptor != stable_descriptor {
                stable_descriptor.clone_from(&probe.descriptor);
                stable_since = Instant::now();
            } else if stable_since.elapsed() >= SEND_STABILITY_INTERVAL {
                return Ok(SendReadiness::Ready(probe));
            }
        } else {
            stable_descriptor.clear();
            if (probe.running || probe.stop_visible) && probe.send_candidate_count == 0 {
                let since = stop_only_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= SEND_STABILITY_INTERVAL {
                    return Ok(SendReadiness::Deferred(probe));
                }
            } else {
                stop_only_since = None;
            }
        }
        if Instant::now() >= deadline {
            return if probe.running || probe.stop_visible || probe.send_candidate_count > 0 {
                Ok(SendReadiness::Deferred(probe))
            } else {
                Ok(SendReadiness::Missing(probe))
            };
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn request_submit(page: &Page, target_url: &str) -> Result<RequestSubmitResult> {
    // Recheck the actual control and invoke it in the same browser task. Native
    // coordinate clicks wait on IntersectionObserver, which may stall in a
    // background tab. A dispatch error remains ambiguous; never click twice.
    let expected = if has_chatgpt_conversation_route(target_url) || !is_chatgpt_url(target_url) {
        target_url
    } else {
        ""
    };
    let expected = serde_json::to_string(expected)?;
    page.evaluate(format!(
        "({REQUEST_SUBMIT_SCRIPT})({{...({SELECT_SEND_CONTROL_SCRIPT})({GENERATION_STOP_CONTROL_SCRIPT}), expected_url:{expected}}}, {GENERATION_STOP_CONTROL_SCRIPT})"
    ))
    .await
    .context("request browser composer submission")?
    .into_value::<RequestSubmitResult>()
    .context("decode browser composer requestSubmit result")
}

pub(super) async fn inspect_submission(
    page: &Page,
    prompt: &str,
    needles: &[String],
) -> Result<SubmissionSnapshot> {
    let payload = serde_json::to_string(&json!({"prompt":prompt,"needles":needles}))?;
    page.evaluate(format!(
        "({INSPECT_SUBMISSION_SCRIPT})({payload}, {GENERATION_STOP_CONTROL_SCRIPT})"
    ))
    .await
    .context("inspect browser prompt submission")?
    .into_value::<SubmissionSnapshot>()
    .context("decode browser prompt submission state")
}

// A request-* container can outlive its optimistic placeholder. A real
// assistant message after the latest user message also proves reception;
// neither an earlier answer nor the empty placeholder does.
fn provisional_submission(snapshot: &SubmissionSnapshot) -> bool {
    snapshot.latest_turn_id.starts_with("request-") && !snapshot.response_started
}

fn weak_submission_evidence(
    baseline: &SubmissionSnapshot,
    current: &SubmissionSnapshot,
) -> Option<&'static str> {
    if provisional_submission(current) {
        return Some("optimistic_echo_unconfirmed");
    }
    if conversation_route_changed(&baseline.url, &current.url) {
        return Some("conversation_url_changed");
    }
    if baseline.composer_contains_prompt && !current.composer_contains_prompt {
        return Some("composer_cleared");
    }
    if !baseline.running && current.running {
        return Some("generation_started");
    }
    None
}

fn verified_submission_kind(
    baseline: &SubmissionSnapshot,
    current: &SubmissionSnapshot,
) -> Option<&'static str> {
    if current.echo_found && !provisional_submission(current) {
        Some("message_echo")
    } else {
        verified_submission_evidence(baseline, current)
    }
}

fn verified_submission_evidence(
    baseline: &SubmissionSnapshot,
    current: &SubmissionSnapshot,
) -> Option<&'static str> {
    (!provisional_submission(current) && current.user_message_count > baseline.user_message_count)
        .then_some("user_message_count_increased")
}

pub(crate) fn stored_verified_submission_evidence(value: &Value) -> Option<&'static str> {
    let baseline = serde_json::from_value(value.get("baseline")?.clone()).ok()?;
    let observed = serde_json::from_value(value.get("observed")?.clone()).ok()?;
    verified_submission_kind(&baseline, &observed)
}

fn conversation_route_changed(before: &str, after: &str) -> bool {
    if before == after {
        return false;
    }
    reqwest::Url::parse(after)
        .ok()
        .and_then(|url| conversation_route_id(&url))
        .is_some_and(|id| !provisional_conversation_id(&id))
}

pub(crate) fn conversation_url_for_target(target: &str, current: &str) -> Option<String> {
    let target = reqwest::Url::parse(target).ok()?;
    let mut current = reqwest::Url::parse(current).ok()?;
    if target.scheme() != current.scheme()
        || target.host_str() != current.host_str()
        || target.port_or_known_default() != current.port_or_known_default()
    {
        return None;
    }
    let conversation_id = conversation_route_id(&current)?;
    if provisional_conversation_id(&conversation_id) || current.path() == target.path() {
        return None;
    }
    current.set_query(None);
    current.set_fragment(None);
    Some(current.to_string())
}

pub(crate) fn is_chatgpt_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.host_str().is_some_and(|host| {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            host == "chatgpt.com" || host.ends_with(".chatgpt.com")
        })
    })
}

pub(crate) fn has_chatgpt_conversation_route(value: &str) -> bool {
    if !is_chatgpt_url(value) {
        return false;
    }
    reqwest::Url::parse(value)
        .ok()
        .and_then(|url| chatgpt_conversation_id(&url))
        .is_some()
}

pub(crate) fn normalized_chatgpt_conversation_url(value: &str) -> Option<String> {
    cccc_core::web_model_connectors::normalized_chatgpt_conversation_url(value)
}

pub(crate) fn conversation_target_matches(expected: &str, observed: &str) -> bool {
    normalized_chatgpt_conversation_url(expected)
        .zip(normalized_chatgpt_conversation_url(observed))
        .is_some_and(|(expected, observed)| expected == observed)
}

fn conversation_route_id(url: &reqwest::Url) -> Option<String> {
    let segments = url.path_segments()?.collect::<Vec<_>>();
    segments.windows(2).find_map(|pair| {
        (matches!(pair[0], "c" | "chat" | "app") && !pair[1].is_empty()).then(|| pair[1].to_owned())
    })
}

fn chatgpt_conversation_id(url: &reqwest::Url) -> Option<String> {
    let segments = url.path_segments()?.collect::<Vec<_>>();
    segments
        .windows(2)
        .find_map(|pair| (pair[0] == "c" && !pair[1].is_empty()).then(|| pair[1].to_owned()))
}

fn provisional_conversation_id(value: &str) -> bool {
    let value = value.trim().to_ascii_uppercase();
    value.starts_with("WEB:") || value.starts_with("WEB%3A")
}

fn submission_needles(prompt: &str) -> Vec<String> {
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Vec::new();
    }
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let mut needles = Vec::new();
    for window in words.windows(3) {
        if window[0].eq_ignore_ascii_case("browser") && window[1].eq_ignore_ascii_case("batch") {
            needles.push(format!("Browser batch {}", window[2]));
            break;
        }
    }
    if let Some(events) = words.iter().find(|word| word.starts_with("events=")) {
        needles.push((*events).to_owned());
    }
    if needles.is_empty() {
        needles.push(normalized.chars().take(120).collect());
    }
    needles
}

fn evidence(
    submitted: bool,
    action: &str,
    submission_evidence: &str,
    input: &str,
    baseline: &SubmissionSnapshot,
    current: &SubmissionSnapshot,
) -> Value {
    json!({
        "submitted":submitted,
        "input_selector":input,
        "send_selector":action,
        "submission_evidence":submission_evidence,
        "tab_url":current.url,
        "baseline":baseline,
        "observed":current
    })
}

fn with_receipt_needles(mut evidence: Value, needles: &[String]) -> Value {
    let receipt_needles = needles
        .iter()
        .filter(|needle| needle.starts_with("Browser batch ") || needle.starts_with("events="))
        .collect::<Vec<_>>();
    if !receipt_needles.is_empty() {
        evidence["receipt_needles"] = json!(receipt_needles);
    }
    evidence
}

fn with_attachment_evidence(
    outcome: PromptSubmissionOutcome,
    attachment: Value,
) -> PromptSubmissionOutcome {
    let attach = |mut value: Value| {
        value["attachment"] = attachment.clone();
        value
    };
    match outcome {
        PromptSubmissionOutcome::Verified(value) => {
            PromptSubmissionOutcome::Verified(attach(value))
        }
        PromptSubmissionOutcome::Deferred(value) => {
            PromptSubmissionOutcome::Deferred(attach(value))
        }
        PromptSubmissionOutcome::Ambiguous(value) => {
            PromptSubmissionOutcome::Ambiguous(attach(value))
        }
    }
}

// Check explicit UI authentication controls, not cookies or text inside a report.
// ponytail: this is a DOM preflight, not proof that the account can call an MCP tool.
// Real tool execution remains the acceptance check when ChatGPT changes its UI.
pub(super) async fn sign_in_required(page: &Page) -> Result<bool> {
    page.evaluate(SIGN_IN_REQUIRED_SCRIPT)
        .await?
        .into_value()
        .context("decode browser sign-in state")
}

const SIGN_IN_REQUIRED_SCRIPT: &str = r#"(() => {
    const candidates = document.querySelectorAll(
        '[data-mobile-auth-entry-action="login"], [data-testid="login-button"], [data-testid="signup-button"], a[href="/auth/login"], a[href^="/auth/login?"], header button, nav button, [role="navigation"] button'
    );
    return Array.from(candidates).some(node => {
        if (node.closest('[data-message-author-role], [data-testid^="conversation-turn"], [inert], [aria-hidden="true"]')) return false;
        const rect=node.getBoundingClientRect(); const style=getComputedStyle(node);
        if (!rect.width || !rect.height || style.display==='none' || style.visibility==='hidden' || Number.parseFloat(style.opacity||'1')<=0.01) return false;
        const explicit=node.matches('[data-mobile-auth-entry-action="login"], [data-testid="login-button"], [data-testid="signup-button"], a[href="/auth/login"], a[href^="/auth/login?"]');
        const label=(node.getAttribute('aria-label')||node.innerText||'').trim();
        return explicit || /^(log in|sign in|sign up|登录|登入|注册|免費註冊|免费注册|ログイン)$/i.test(label);
    });
})()"#;

pub(super) const SELECT_COMPOSER_SCRIPT: &str = r#"() => {
    const markerName = 'data-cccc-web-model-composer';
    const markerValue = 'cccc-web-model-composer';
    const visible = node => {
        if (!node || node.closest('[aria-hidden="true"], [inert]')) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width >= 16 && rect.height >= 16
            && rect.bottom > 0 && rect.right > 0
            && rect.top < innerHeight && rect.left < innerWidth
            && style.display !== 'none' && style.visibility !== 'hidden'
            && Number.parseFloat(style.opacity || '1') > 0.01;
    };
    const editable = node => {
        if (!visible(node)) return false;
        if (node instanceof HTMLTextAreaElement || node instanceof HTMLInputElement) {
            const type = String(node.type || 'text').toLowerCase();
            return !node.disabled && !node.readOnly
                && !/password|search|email|url|number|tel|file|hidden|checkbox|radio|submit|button|reset/.test(type);
        }
        return node.isContentEditable || node.getAttribute('contenteditable') === 'true';
    };
    const label = node => [
        node.getAttribute('aria-label') || '', node.getAttribute('placeholder') || '',
        node.getAttribute('name') || '', node.id || '', node.getAttribute('data-testid') || '',
        node.className || ''
    ].join(' ').toLowerCase();
    const score = node => {
        const rect = node.getBoundingClientRect();
        const text = label(node);
        let value = 0;
        if (node.id === 'prompt-textarea') value += 240;
        if (String(node.className || '').includes('ProseMirror')) value += 180;
        if (/prompt|message|ask|chat|query|composer/.test(text)) value += 120;
        if (node.isContentEditable || node.getAttribute('contenteditable') === 'true') value += 70;
        if (node.getAttribute('role') === 'textbox') value += 45;
        if (node.closest('form')) value += 55;
        if (node.closest('main')) value += 25;
        if (document.activeElement === node) value += 30;
        if (/fallback|search|filter/.test(text)) value -= 220;
        if (rect.width >= 260 && rect.height >= 26) value += 40;
        value += Math.min(100, Math.max(0, rect.width * rect.height / 3000));
        value += Math.max(0, rect.top / 20);
        return value;
    };
    const nodes = [];
    const seen = new Set();
    const add = node => {
        if (!node || seen.has(node)) return;
        seen.add(node);
        nodes.push(node);
    };
    const selectors = [
        '.ProseMirror', '#prompt-textarea', '[contenteditable="true"][data-virtualkeyboard="true"]',
        '[role="textbox"][contenteditable="true"]', 'textarea[data-id="prompt-textarea"]',
        'textarea[name="prompt-textarea"]', 'textarea[placeholder*="Send a message"]',
        'textarea[aria-label*="Chat"]', 'main [contenteditable="true"]', 'main textarea',
        '[contenteditable="true"]', 'textarea', 'input'
    ];
    for (const selector of selectors) {
        try { document.querySelectorAll(selector).forEach(add); } catch (_) {}
    }
    const best = nodes.filter(editable).sort((left, right) => score(right) - score(left))[0];
    document.querySelectorAll(`[${markerName}]`).forEach(node => node.removeAttribute(markerName));
    if (!best) return { selector: '', descriptor: '' };
    best.setAttribute(markerName, markerValue);
    const descriptor = [best.tagName.toLowerCase(), best.id ? `#${best.id}` : '',
        best.getAttribute('role') ? `[role=${best.getAttribute('role')}]` : '',
        best.isContentEditable ? '[contenteditable=true]' : ''].join('');
    return { selector: `[${markerName}="${markerValue}"]`, descriptor };
}"#;

const FOCUS_AND_SELECT_SCRIPT: &str = r#"() => {
    const input = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    if (!input) return false;
    input.focus();
    if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) {
        input.setSelectionRange(0, String(input.value || '').length);
    } else {
        const range = document.createRange();
        range.selectNodeContents(input);
        const selection = getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
    }
    return document.activeElement === input;
}"#;

// Share the actual control predicate across readiness, Send and final form submission.
const GENERATION_STOP_CONTROL_SCRIPT: &str = r#"node => {
    if (node.closest('nav, aside, [role="navigation"], [data-message-author-role], [data-testid^="conversation-turn"], main article')
        || node.hasAttribute('data-conversation-options-trigger')) return false;
    if (node.getAttribute('data-testid') === 'stop-button') return true;
    const action = String(node.getAttribute('aria-label') || node.getAttribute('title')
        || node.innerText || node.textContent || '').replace(/\s+/g, ' ').trim().toLowerCase();
    return /^(?:stop(?: streaming| generating| generation)?|停止(?:生成|回答|响应|回复|流式传输)?|中止|生成を停止|応答を停止|cancel generation|interrupt)$/.test(action);
}"#;

const SELECT_SEND_CONTROL_SCRIPT: &str = r#"isGenerationStop => {
    const input = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    const markerName = 'data-cccc-web-model-send';
    const markerValue = 'cccc-web-model-send';
    document.querySelectorAll(`[${markerName}]`).forEach(node => node.removeAttribute(markerName));
    const visible = node => {
        if (!node || node.closest('[aria-hidden="true"], [inert]')) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width >= 8 && rect.height >= 8 && style.display !== 'none'
            && style.visibility !== 'hidden' && Number.parseFloat(style.opacity || '1') > 0.01;
    };
    const disabled = node => Boolean(node.disabled)
        || String(node.getAttribute('aria-disabled') || '').toLowerCase() === 'true';
    const label = node => [node.getAttribute('aria-label') || '', node.getAttribute('title') || '',
        node.getAttribute('data-testid') || '', node.id || '', node.className || '',
        node.innerText || node.textContent || ''].join(' ').replace(/\s+/g, ' ').trim().toLowerCase();
    const stop = isGenerationStop;
    const unsafe = node => stop(node)
        || /retry|signin|sign in|log in|login|voice|dictat|microphone|attach|upload|google|microsoft|apple/.test(label(node));
    const sendLike = node => {
        const text = label(node);
        return node.id === 'composer-submit-button'
            || /(^|[-_])send([-_]|$)/.test(String(node.getAttribute('data-testid') || '').toLowerCase())
            || node.getAttribute('type') === 'submit'
            || /\bsend\b|\bsubmit\b|发送|送信/.test(text);
    };
    const allButtons = Array.from(document.querySelectorAll('button, [role="button"]')).filter(visible);
    const running = allButtons.some(stop);
    let root = input?.closest('form')
        || input?.closest('[data-testid*="composer" i], [class*="composer" i]')
        || null;
    if (!root && input) {
        let parent = input.parentElement;
        for (let depth = 0; parent && depth < 6; depth += 1, parent = parent.parentElement) {
            if (parent.querySelector('button, [role="button"]')) { root = parent; break; }
        }
    }
    const buttons = root ? Array.from(root.querySelectorAll('button, [role="button"]')).filter(visible) : [];
    const sendCandidates = buttons.filter(node => sendLike(node) && !unsafe(node));
    const enabled = sendCandidates.filter(node => !disabled(node));
    const score = node => {
        const text = label(node);
        let value = 0;
        if (node.id === 'composer-submit-button') value += 220;
        if (String(node.getAttribute('data-testid') || '').toLowerCase() === 'send-button') value += 220;
        if (node.getAttribute('type') === 'submit') value += 80;
        if (/send prompt|发送|送信/.test(text)) value += 130;
        if (/\bsend\b|\bsubmit\b/.test(text)) value += 80;
        return value;
    };
    const best = enabled.sort((left, right) => score(right) - score(left))[0];
    if (!best) return {
        selector: '', descriptor: '', running, stop_visible: running,
        send_candidate_count: sendCandidates.length
    };
    best.setAttribute(markerName, markerValue);
    const descriptor = best.id ? `#${best.id}`
        : best.getAttribute('data-testid') ? `[data-testid=${best.getAttribute('data-testid')}]`
        : best.getAttribute('aria-label') ? `[aria-label=${best.getAttribute('aria-label')}]`
        : best.getAttribute('type') === 'submit' ? 'button[type=submit]' : 'composer:send-control';
    return {
        selector: `[${markerName}="${markerValue}"]`, descriptor, running,
        stop_visible: running, send_candidate_count: sendCandidates.length
    };
}"#;

const REQUEST_SUBMIT_SCRIPT: &str = r#"(probe, isGenerationStop) => {
    const route = value => { const u = new URL(value, location.href); u.search=''; u.hash=''; return u.href; };
    if (probe.expected_url && route(probe.expected_url) !== route(location.href)) {
        return {action:'none:target_changed',invoked:false,unsafe_state:true,error:'conversation target changed'};
    }
    if (probe.running || probe.stop_visible) {
        return { action: '', invoked: false, unsafe_state: true, error: '' };
    }
    const selected = probe.selector ? document.querySelector(probe.selector) : null;
    if (selected) {
        const action = `dom.click:${probe.descriptor}`;
        try {
            selected.click();
            return { action, invoked: true, unsafe_state: false, error: '' };
        } catch (error) {
            return { action, invoked: true, unsafe_state: false, error: String(error || '') };
        }
    }
    const input = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    const form = input?.closest('form') || null;
    if (!form || typeof form.requestSubmit !== 'function') {
        return { action: '', invoked: false, unsafe_state: false, error: '' };
    }
    const visible = node => {
        if (!node) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden';
    };
    const label = node => [node.getAttribute('aria-label') || '', node.getAttribute('title') || '',
        node.getAttribute('data-testid') || '', node.id || '', node.innerText || node.textContent || '']
        .join(' ').replace(/\s+/g, ' ').trim().toLowerCase();
    const buttons = Array.from(form.querySelectorAll('button, [role="button"]')).filter(visible);
    if (buttons.some(isGenerationStop)) {
        return { action: '', invoked: false, unsafe_state: true, error: '' };
    }
    const submit = buttons.find(node => !node.disabled
        && String(node.getAttribute('aria-disabled') || '').toLowerCase() !== 'true'
        && (node.getAttribute('type') === 'submit' || /\bsend\b|\bsubmit\b|发送|送信/.test(label(node))));
    try {
        form.requestSubmit(submit || undefined);
        return { action: submit ? 'form.requestSubmit:button' : 'form.requestSubmit', invoked: true, unsafe_state: false, error: '' };
    } catch (error) {
        return { action: 'form.requestSubmit', invoked: true, unsafe_state: false, error: String(error || '') };
    }
}"#;

const INSPECT_SUBMISSION_SCRIPT: &str = r#"(payload, isGenerationStop) => {
    const normalize = value => String(value || '').replace(/\u00a0/g, ' ').replace(/\s+/g, ' ').trim();
    const expected = normalize(payload.prompt);
    const prefix = expected.slice(0, 160);
    const suffix = expected.slice(-120);
    const containsPrompt = text => {
        const actual = normalize(text);
        if (!actual || !expected) return false;
        if (actual.includes(expected)) return true;
        return Boolean(prefix && actual.includes(prefix)) && (expected.length <= 200 || Boolean(suffix && actual.includes(suffix)));
    };
    const visible = node => {
        if (!node || node.closest('[aria-hidden="true"], [inert]')) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width > 0 && rect.height > 0 && style.display !== 'none'
            && style.visibility !== 'hidden' && Number.parseFloat(style.opacity || '1') > 0.01;
    };
    const editable = node => visible(node) && (
        ((node instanceof HTMLTextAreaElement || node instanceof HTMLInputElement) && !node.disabled && !node.readOnly)
        || node.isContentEditable || node.getAttribute('contenteditable') === 'true'
    );
    const read = node => normalize('value' in node ? node.value : (node.innerText || node.textContent || ''));
    const marked = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    const markedText = marked ? read(marked) : '';
    const composers = Array.from(new Set([
        ...(marked ? [marked] : []),
        ...document.querySelectorAll('main textarea, main [role="textbox"], main [contenteditable="true"]'),
        ...document.querySelectorAll('textarea, [role="textbox"], [contenteditable="true"]')
    ])).filter(editable);
    const composerTexts = composers.map(read).filter(Boolean);
    const messageNodes = Array.from(new Set([
        ...document.querySelectorAll('[data-message-author-role="user"]'),
        ...document.querySelectorAll('[data-testid*="conversation-turn"]'),
        ...document.querySelectorAll('main article')
    ]));
    const needles = Array.isArray(payload.needles) ? payload.needles.map(normalize).filter(Boolean) : [];
    const echoFound = messageNodes.some(node => {
        const text = read(node);
        return needles.some(needle => text.includes(needle));
    });
    const messages = Array.from(document.querySelectorAll('[data-message-author-role]'));
    const lastUser = messages.findLastIndex(node => node.getAttribute('data-message-author-role') === 'user');
    const responseStarted = lastUser >= 0 && messages.slice(lastUser + 1).some(node =>
        node.getAttribute('data-message-author-role') === 'assistant'
        && Boolean(node.getAttribute('data-message-id')) && read(node).length > 0);
    const controls = Array.from(document.querySelectorAll('button, [role="button"]')).filter(visible);
    const label = node => [node.getAttribute('aria-label') || '', node.getAttribute('title') || '',
        node.getAttribute('data-testid') || '', node.id || '', node.innerText || node.textContent || '']
        .join(' ').replace(/\s+/g, ' ').trim().toLowerCase();
    const stopVisible = controls.some(isGenerationStop);
    const chromeUI = node => !node.closest('nav, aside, [role="navigation"], [data-message-author-role], [data-testid^="conversation-turn"], main article');
    const notices = Array.from(document.querySelectorAll('[role="alert"], [role="status"], [role="dialog"], [data-testid*="error"], #challenge-stage'))
        .filter(node => visible(node) && chromeUI(node)).map(read).join(' ');
    const archived = !composers.length && controls.some(node => chromeUI(node)
        && /^(?:unarchive(?: conversation| chat)?|取消归档|取消封存|解除封存|アーカイブ解除)$/i.test(normalize(node.getAttribute('aria-label') || node.innerText || node.textContent)));
    const blocker = /too many requests|rate limit|操作(?:过于|太)?频繁|请求过多|请求过于频繁/i.test(notices) ? 'rate_limited'
        : /access denied|request blocked|访问被拒绝|拒绝访问/i.test(notices) ? 'access_denied'
        : /verify (?:you are|you're) human|verifying you are human|确认您是真人|验证您是人类|验证您是否为人类/i.test(notices) ? 'verification_required'
        : archived ? 'conversation_archived' : '';
    const safeSend = controls.filter(node => {
        const text = label(node);
        if (/\bstop\b|停止|中止|cancel|retry|signin|sign in|log in|login|voice|microphone|attach|upload/.test(text)) return false;
        return node.id === 'composer-submit-button'
            || /(^|[-_])send([-_]|$)/.test(String(node.getAttribute('data-testid') || '').toLowerCase())
            || node.getAttribute('type') === 'submit' || /\bsend\b|\bsubmit\b|发送|送信/.test(text);
    });
    return {
        url: location.href || '', echo_found: echoFound, running: stopVisible, stop_visible: stopVisible, page_blocker: blocker,
        response_started: responseStarted,
        composer_exact: Boolean(markedText && markedText === expected),
        composer_contains_prompt: composerTexts.some(containsPrompt),
        // Before selecting an input, existing visible drafts still prevent a target switch.
        composer_chars: marked ? markedText.length : Math.max(0, ...composerTexts.map(text => text.length)),
        latest_turn_id: [...document.querySelectorAll('[data-testid^="conversation-turn"], main article')].at(-1)?.getAttribute('data-turn-id') || '',
        user_message_count: document.querySelectorAll('[data-message-author-role="user"]').length,
        send_enabled_count: safeSend.filter(node => !node.disabled
            && String(node.getAttribute('aria-disabled') || '').toLowerCase() !== 'true').length
    };
}"#;

const DISMISS_DUPLICATE_UPLOAD_DIALOG_SCRIPT: &str = r#"() => {
    const visible = node => {
        if (!node || node.closest('[aria-hidden="true"], [inert]')) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width > 0 && rect.height > 0 && style.display !== 'none'
            && style.visibility !== 'hidden' && Number.parseFloat(style.opacity || '1') > 0.01;
    };
    const duplicate = Array.from(document.querySelectorAll('[role="dialog"], [aria-modal="true"]'))
        .filter(visible)
        .find(node => {
            const text = String(node.innerText || node.textContent || '').replace(/\s+/g, ' ').toLowerCase();
            return text.includes("you've already uploaded this file")
                || text.includes('you have already uploaded this file');
        });
    if (!duplicate) return 'none';
    const button = Array.from(duplicate.querySelectorAll('button, [role="button"]'))
        .filter(visible)
        .find(node => /^(ok|okay|确定|確認)$/.test(
            String(node.innerText || node.textContent || node.getAttribute('aria-label') || '').trim().toLowerCase()
        ));
    if (!button) return 'blocked';
    button.click();
    return 'dismissed';
}"#;

const DUPLICATE_UPLOAD_DIALOG_VISIBLE_SCRIPT: &str = r#"() => {
    const visible = node => {
        if (!node || node.closest('[aria-hidden="true"], [inert]')) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width > 0 && rect.height > 0 && style.display !== 'none'
            && style.visibility !== 'hidden' && Number.parseFloat(style.opacity || '1') > 0.01;
    };
    return Array.from(document.querySelectorAll('[role="dialog"], [aria-modal="true"]'))
        .filter(visible)
        .some(node => {
            const text = String(node.innerText || node.textContent || '').replace(/\s+/g, ' ').toLowerCase();
            return text.includes("you've already uploaded this file")
                || text.includes('you have already uploaded this file');
        });
}"#;

const ATTACHMENT_STATUS_SCRIPT: &str = r#"payload => {
    const visible = node => {
        if (!node || node.closest('[aria-hidden="true"], [inert]')) return false;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return rect.width >= 8 && rect.height >= 8 && style.display !== 'none'
            && style.visibility !== 'hidden' && Number.parseFloat(style.opacity || '1') > 0.01;
    };
    const input = document.querySelector('#upload-photos, input[type=file][accept*="image"]');
    const fileCount = input?.files?.length || 0;
    const filename = String(payload?.filename || '');
    const ownedInput = fileCount >= 1 && Array.from(input.files || []).some(file => file?.name === filename);
    const composer = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    const root = composer?.closest('form')
        || composer?.closest('[data-testid*="composer" i], [class*="composer" i]')
        || document;
    const explicitPreviews = Array.from(root.querySelectorAll(
        '[data-testid*="attachment" i], [data-testid*="file-preview" i], [data-testid*="upload-preview" i], '
        + '[class*="attachment" i], [class*="file-preview" i], [class*="upload-preview" i]'
    )).filter(node => !(node instanceof HTMLInputElement) && visible(node));
    const imagePreviews = Array.from(root.querySelectorAll('img')).filter(node => {
        if (!visible(node)) return false;
        const rect = node.getBoundingClientRect();
        return rect.width >= 24 && rect.height >= 24 && rect.width <= 512 && rect.height <= 512;
    });
    const previewNodes = Array.from(new Set([...explicitPreviews, ...imagePreviews]));
    const ownedPreview = previewNodes.some(node => [node.getAttribute('aria-label') || '', node.getAttribute('alt') || '',
        node.getAttribute('title') || '', node.textContent || ''].join(' ').includes(filename));
    const deliveryId = String(payload?.delivery_id || '');
    const marked = document.documentElement.dataset.ccccWebModelAttachmentDelivery === deliveryId;
    const dispatched = document.documentElement.dataset.ccccWebModelAttachmentDispatched === deliveryId;
    return { ready: marked || ownedInput || ownedPreview || previewNodes.length > 0, marked, dispatched,
        owned_input: ownedInput, owned_preview: ownedPreview, file_count: fileCount,
        preview_count: previewNodes.length, image_preview_count: imagePreviews.length };
}"#;

const OBSERVE_STAGED_PROMPT_SCRIPT: &str = r#"payload => {
    const input = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    if (!input) return;
    if (input.__ccccStagingObserver) {
        input.removeEventListener('input', input.__ccccStagingObserver);
        delete input.__ccccStagingObserver;
    }
    if (!payload.install) return;
    const normalize = text => String(text).replace(/\s+/g, ' ').trim();
    const observe = event => {
        const text = input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement
            ? input.value : (input.innerText || input.textContent || '');
        if (event.isTrusted && event.inputType === 'insertText'
            && normalize(text) === normalize(payload.prompt)) {
            input.__ccccStagedPrompt = {deliveryId:payload.delivery_id, text};
        }
    };
    input.__ccccStagingObserver = observe;
    input.addEventListener('input', observe, {once:true});
}"#;

const CLEAR_STAGED_PROMPT_SCRIPT: &str = r#"payload => {
    const input = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    const owned = input?.__ccccStagedPrompt;
    if (!owned || owned.deliveryId !== payload.delivery_id) return true;
    const normalize = text => String(text).replace(/\s+/g, ' ').trim();
    if (normalize(owned.text) !== normalize(payload.prompt)) return true;
    const text = () => input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement
        ? input.value : (input.innerText || input.textContent || '');
    if (text() !== owned.text) return true;
    input.focus();
    if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) {
        input.setSelectionRange(0, input.value.length);
    } else {
        const range = document.createRange();
        range.selectNodeContents(input);
        const selection = getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
    }
    document.execCommand('delete', false);
    if (normalize(text()) !== '') return false;
    delete input.__ccccStagedPrompt;
    return true;
}"#;
