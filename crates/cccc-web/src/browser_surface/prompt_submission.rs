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
    verification_required: bool,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct SendProbe {
    selector: String,
    descriptor: String,
    running: bool,
    stop_visible: bool,
    send_candidate_count: usize,
    point: Option<SendPoint>,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
struct SendPoint {
    x: f64,
    y: f64,
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
struct SubmissionSnapshot {
    url: String,
    echo_found: bool,
    running: bool,
    stop_visible: bool,
    composer_exact: bool,
    composer_contains_prompt: bool,
    composer_chars: usize,
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
    /// Observe a late/manual send without navigating, touching the draft or
    /// invoking Send. Only the exact batch in a user message is a receipt.
    pub(crate) async fn inspect_delivery_receipt(
        &self,
        key: &str,
        target_url: &str,
        marker: &str,
    ) -> Result<Option<Value>> {
        if target_url.is_empty() || marker.is_empty() {
            return Ok(None);
        }
        let page = self.page(key).await?;
        let payload = serde_json::to_string(&json!({
            "prompt":"", "needles":[marker]
        }))?;
        let observed = page
            .evaluate(format!("({INSPECT_SUBMISSION_SCRIPT})({payload})"))
            .await?
            .into_value::<SubmissionSnapshot>()?;
        let same_target = if is_chatgpt_url(target_url)
            || cccc_core::web_model_connectors::grok_bot_url(target_url).is_ok()
        {
            conversation_target_matches(target_url, &observed.url)
        } else {
            target_url == observed.url
        };
        if !same_target
            || !observed.echo_found
            || self.page(key).await?.target_id() != page.target_id()
        {
            return Ok(None);
        }
        Ok(Some(json!({
            "submitted":true, "submission_evidence":"message_echo",
            "send_selector":"", "observed":observed,
            "recovered_from":"submission_ambiguous"
        })))
    }

    pub(crate) async fn inspect_staged_prompt(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
    ) -> Result<Value> {
        let page = self.page(key).await?;
        let _ = wait_for_composer(&page).await?;
        let snapshot = inspect_submission(&page, prompt, &submission_needles(prompt)).await?;
        let recoverable = recoverable_staged_draft(target_url, &snapshot);
        self.record_page_state(key, &page).await;
        Ok(json!({
            "recoverable":recoverable,
            "expected_target_url":target_url,
            "observed":snapshot
        }))
    }

    pub(crate) async fn submit_prompt_with_attachment(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
        attachment_path: Option<&Path>,
        delivery_id: &str,
        expected_staged_draft: Option<&str>,
    ) -> Result<PromptSubmissionOutcome> {
        self.submit_prompt(
            key,
            target_url,
            prompt,
            attachment_path,
            delivery_id,
            expected_staged_draft,
            None,
        )
        .await
    }

    pub(crate) async fn submit_pairing_prompt(
        &self,
        key: &str,
        target: &super::PairingPage,
        prompt: &str,
    ) -> Result<PromptSubmissionOutcome> {
        self.submit_prompt(key, "", prompt, None, "", None, Some(target))
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn submit_prompt(
        &self,
        key: &str,
        target_url: &str,
        prompt: &str,
        attachment_path: Option<&Path>,
        delivery_id: &str,
        expected_staged_draft: Option<&str>,
        pairing: Option<&super::PairingPage>,
    ) -> Result<PromptSubmissionOutcome> {
        if prompt.trim().is_empty() {
            bail!("browser prompt is empty");
        }
        let page = self.page(key).await?;
        if let Some(target) = pairing {
            target.check_before_send(self, key).await?;
        }
        // Recovery owns one exact draft on the current page, not permission to
        // navigate back and replace whatever is there after a user changes tabs.
        if expected_staged_draft.is_none() {
            if has_chatgpt_conversation_route(target_url) {
                self.align_chatgpt_conversation_target(key, target_url, Duration::from_secs(5))
                    .await?;
            } else if !target_url.is_empty() {
                let current = page.url().await?.unwrap_or_default();
                if !same_page(&current, target_url) {
                    goto_dom_content_loaded(&page, target_url).await?;
                }
            }
        }
        dismiss_duplicate_upload_dialog(&page).await?;

        // Pairing retries share their explanatory prefix, but each carries a
        // different code. Only the complete setup message identifies this send.
        let needles = if pairing.is_some() {
            vec![prompt.to_owned()]
        } else {
            submission_needles(prompt)
        };
        let existing = inspect_submission(&page, prompt, &needles).await?;
        if existing.echo_found && expected_staged_draft.is_none() {
            self.record_page_state(key, &page).await;
            return Ok(PromptSubmissionOutcome::Verified(evidence(
                true,
                "existing:message_echo",
                "message_echo",
                "",
                &existing,
                &existing,
            )));
        }

        let composer = wait_for_composer(&page).await?;
        let staged = inspect_submission(&page, prompt, &needles).await?;
        let owns_staged_draft = if let Some(expected) = expected_staged_draft {
            let previous =
                inspect_submission(&page, expected, &submission_needles(expected)).await?;
            if !recoverable_staged_draft(target_url, &previous) {
                return Ok(PromptSubmissionOutcome::Ambiguous(evidence(
                    false,
                    "",
                    "staged_draft_changed",
                    &composer.descriptor,
                    &previous,
                    &previous,
                )));
            }
            true
        } else {
            false
        };
        if !staged.composer_exact && staged.composer_chars > 0 && !owns_staged_draft {
            // A previous delivery or a human may own this draft. Never replace
            // it, including if it appeared after the pre-claim readiness check.
            return Ok(PromptSubmissionOutcome::Ambiguous(evidence(
                false,
                "",
                "composer_occupied",
                &composer.descriptor,
                &staged,
                &staged,
            )));
        }
        if !staged.composer_exact {
            if let Some(target) = pairing {
                target.check_before_send(self, key).await?;
            }
            focus_and_select_composer(&page).await?;
            page.execute(InsertTextParams::new(prompt))
                .await
                .context("insert prompt into visible browser composer")?;
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
        if let Some(target) = pairing {
            target.check_before_send(self, key).await?;
        }
        if self.page(key).await?.target_id() != page.target_id() {
            bail!("browser surface changed before prompt submission");
        }
        // Waiting for controls must not grant permission to send a draft that
        // a human changed in the meantime (or submit a manual send twice).
        let before_send = inspect_submission(&page, prompt, &needles).await?;
        if before_send.echo_found || !before_send.composer_exact {
            return Ok(self
                .classify_deferred_submission(
                    key,
                    &page,
                    SubmissionAttempt {
                        prompt,
                        needles: &needles,
                        input: &composer.descriptor,
                        action: "",
                        baseline: &baseline,
                    },
                    "staged_draft_changed",
                )
                .await);
        }
        match readiness {
            SendReadiness::Ready(candidate) => {
                // Activate the validated control in its own document. Native
                // pointer dispatch can acknowledge without delivering a click
                // to a background window. This still uses the page's normal
                // click/form handlers, exactly once; it is not a retry path.
                let activation =
                    activate_send_control(&page, &candidate, prompt, &needles, &before_send.url)
                        .await;
                if matches!(&activation, Ok(result) if !result.invoked) {
                    return Ok(self
                        .classify_deferred_submission(
                            key,
                            &page,
                            SubmissionAttempt {
                                prompt,
                                needles: &needles,
                                input: &composer.descriptor,
                                action: "",
                                baseline: &baseline,
                            },
                            "send_control_changed",
                        )
                        .await);
                }
                let action = match activation {
                    Ok(result) if result.error.is_empty() => result.action,
                    _ => format!("{}:dom_click:dispatch_unknown", candidate.descriptor),
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
            SendReadiness::Missing(probe) => {
                let request_submit = request_submit(&page).await;
                match request_submit {
                    Ok(result) if result.unsafe_state => Ok(self
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
                        .await),
                    Ok(result) if result.invoked => {
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
                    Ok(_) if probe.running || probe.stop_visible => Ok(self
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
                    Ok(_) => {
                        let action = "keyboard:Enter";
                        let press = match page.find_element(COMPOSER_SELECTOR).await {
                            Ok(input) => input.press_key("Enter").await.map(|_| ()),
                            Err(error) => Err(error),
                        };
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
                    Err(_) => Ok(self
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
                        .await),
                }
            }
        }
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
        if observed.echo_found {
            return PromptSubmissionOutcome::Verified(evidence(
                true,
                attempt.action,
                "message_echo",
                attempt.input,
                attempt.baseline,
                &observed,
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
                    if snapshot.echo_found {
                        self.record_page_state(key, page).await;
                        return PromptSubmissionOutcome::Verified(evidence(
                            true,
                            action,
                            "message_echo",
                            input,
                            baseline,
                            &snapshot,
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

    async fn page(&self, key: &str) -> Result<Page> {
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

    pub(crate) async fn prompt_readiness(&self, key: &str) -> Result<Value> {
        let page = self.page(key).await?;
        let url = page.url().await?.unwrap_or_default();
        // A new window may still be waiting for response headers. It cannot
        // contain a composer, and evaluating JS there can wait for navigation.
        let loading = matches!(url.as_str(), "" | "about:blank");
        let candidate = if loading {
            ComposerCandidate::default()
        } else {
            page.evaluate(format!("({SELECT_COMPOSER_SCRIPT})()"))
                .await
                .context("inspect visible browser composer")?
                .into_value::<ComposerCandidate>()
                .context("decode visible browser composer")?
        };
        let ready = !candidate.selector.is_empty();
        let snapshot = if ready {
            inspect_submission(&page, "", &[]).await?
        } else {
            SubmissionSnapshot::default()
        };
        let readiness = json!({
            "ready":ready,
            "composer_chars":snapshot.composer_chars,
            "composer_has_attachments":!loading && composer_has_attachments(&page).await?,
            "running":snapshot.running,
            "login_required":!ready && !loading,
            "verification_required":candidate.verification_required,
            "tab_url":url,
            "input_selector":candidate.descriptor,
            "checked_at":cccc_contracts::utc_now(),
            "message":if loading {
                "Browser page is loading. Delivery is waiting."
            } else if candidate.verification_required {
                "Complete the website's security verification in this browser. Delivery is waiting."
            } else if ready {
                "Browser model composer is ready."
            } else {
                "Browser model sign-in or composer setup is required."
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

// Keep the early recovery inspection and final replacement check identical.
fn recoverable_staged_draft(target_url: &str, snapshot: &SubmissionSnapshot) -> bool {
    same_page(&snapshot.url, target_url)
        && snapshot.user_message_count == 0
        && !snapshot.echo_found
        && !snapshot.running
        && !snapshot.stop_visible
        && snapshot.composer_exact
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
    let mut stable_point = None;
    let mut stable_since = Instant::now();
    let mut stop_only_since = None;
    loop {
        let probe = inspect_send_control(page).await?;
        if !probe.selector.is_empty() {
            if probe.descriptor != stable_descriptor || probe.point != stable_point {
                stable_descriptor.clone_from(&probe.descriptor);
                stable_point = probe.point;
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

async fn inspect_send_control(page: &Page) -> Result<SendProbe> {
    page.evaluate(format!("({SELECT_SEND_CONTROL_SCRIPT})()"))
        .await
        .context("inspect browser composer send control")?
        .into_value::<SendProbe>()
        .context("decode browser composer send control")
}

async fn activate_send_control(
    page: &Page,
    candidate: &SendProbe,
    prompt: &str,
    needles: &[String],
    url: &str,
) -> Result<RequestSubmitResult> {
    let payload = json!({
        "prompt":prompt, "needles":needles, "url":url,
        "descriptor":candidate.descriptor
    });
    // Reuse the same receipt, draft and visible-control predicates. All final
    // checks and activation run in one JS task, so a draft edit, manual send,
    // navigation or disabled/covered button cannot slip between CDP calls.
    page.evaluate(format!(
        r#"(() => {{
            const payload = {payload};
            const blocked = {{action:'', invoked:false, unsafe_state:true, error:''}};
            const state = ({INSPECT_SUBMISSION_SCRIPT})(payload);
            if (state.url !== payload.url || state.echo_found || !state.composer_exact) return blocked;
            const current = ({SELECT_SEND_CONTROL_SCRIPT})();
            if (!current.selector || current.descriptor !== payload.descriptor) return blocked;
            const button = document.querySelector(current.selector);
            if (!(button instanceof HTMLElement)) return blocked;
            const result = {{action:current.descriptor + ':dom_click', invoked:true, unsafe_state:false, error:''}};
            try {{ button.click(); }} catch (error) {{ result.error = String(error || ''); }}
            return result;
        }})()"#,
    ))
    .await
    .context("activate verified browser send control")?
    .into_value::<RequestSubmitResult>()
    .context("decode browser send activation")
}

async fn request_submit(page: &Page) -> Result<RequestSubmitResult> {
    page.evaluate(format!("({REQUEST_SUBMIT_SCRIPT})()"))
        .await
        .context("request browser composer submission")?
        .into_value::<RequestSubmitResult>()
        .context("decode browser composer requestSubmit result")
}

async fn inspect_submission(
    page: &Page,
    prompt: &str,
    needles: &[String],
) -> Result<SubmissionSnapshot> {
    let payload = serde_json::to_string(&json!({"prompt":prompt,"needles":needles}))?;
    page.evaluate(format!("({INSPECT_SUBMISSION_SCRIPT})({payload})"))
        .await
        .context("inspect browser prompt submission")?
        .into_value::<SubmissionSnapshot>()
        .context("decode browser prompt submission state")
}

fn weak_submission_evidence(
    baseline: &SubmissionSnapshot,
    current: &SubmissionSnapshot,
) -> Option<&'static str> {
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
    if !is_chatgpt_url(value) {
        return None;
    }
    let mut url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let conversation_id = chatgpt_conversation_id(&url)?;
    if provisional_conversation_id(&conversation_id) {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

pub(crate) fn conversation_target_matches(expected: &str, observed: &str) -> bool {
    if let Ok(expected) = cccc_core::web_model_connectors::grok_bot_url(expected) {
        return cccc_core::web_model_connectors::grok_bot_url(observed)
            .is_ok_and(|observed| expected == observed);
    }
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

pub(super) fn provisional_conversation_id(value: &str) -> bool {
    let value = value.trim().to_ascii_uppercase();
    value.starts_with("WEB:") || value.starts_with("WEB%3A")
}

fn submission_needles(prompt: &str) -> Vec<String> {
    // One complete transport header identifies the batch, its source events
    // and Actor. An event list or shared prose prefix alone is not a receipt.
    let receipt = prompt
        .lines()
        .find(|line| line.starts_with("[cccc] Browser batch "))
        .unwrap_or(prompt);
    let normalized = receipt.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Vec::new();
    }
    vec![normalized]
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

const SELECT_COMPOSER_SCRIPT: &str = r#"() => {
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
    // A provider challenge page is not a composer. Ordinary background challenge
    // scripts do not imply that the user is being asked to complete verification.
    if (document.querySelector('script[src*="/challenge-platform/"][src*="/orchestrate/chl_page/"]')) {
        return { selector: '', descriptor: '', verification_required: true };
    }
    // ChatGPT exposes a guest composer too; its presence does not prove sign-in.
    if (Array.from(document.querySelectorAll('[data-testid="login-button"]')).some(visible)) {
        return { selector: '', descriptor: '' };
    }
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
    // ChatGPT's landing/auth pages contain unrelated editable fields. Only its
    // conversation composer is a delivery target; generic inputs are not.
    const chatGptHost = /(^|\.)chatgpt\.com$/.test(location.hostname)
        || ['chat.openai.com', 'auth.openai.com'].includes(location.hostname);
    const selectors = chatGptHost ? ['#prompt-textarea'] : location.hostname === 'grok.com' ? ['[data-testid="chat-input"] [role="textbox"][contenteditable="true"]'] : [
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

const SELECT_SEND_CONTROL_SCRIPT: &str = r#"() => {
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
    const stop = node => /\bstop\b|停止|中止|cancel generation|interrupt/.test(label(node));
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
    const working = document.querySelector('[data-testid="bot-working-slot"]');
    const running = allButtons.some(stop) || !!working && Array.from(working.children).some(visible);
    let root = input?.closest('form')
        || input?.closest('[data-testid*="composer" i], [class*="composer" i]')
        || null;
    if (!root && input) {
        let parent = input.parentElement;
        for (let depth = 0; parent && depth < 6; depth += 1, parent = parent.parentElement) {
            if (parent.querySelector(input.closest('[data-testid="chat-input"]') ? '[data-testid="chat-submit"]' : 'button, [role="button"]')) { root = parent; break; }
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
    if (!best || running) return {
        selector: '', descriptor: '', running, stop_visible: running,
        send_candidate_count: sendCandidates.length
    };
    let rect = best.getBoundingClientRect();
    if (rect.top < 0 || rect.left < 0 || rect.bottom > innerHeight || rect.right > innerWidth) {
        best.scrollIntoView({block:'nearest', inline:'nearest', behavior:'instant'});
        rect = best.getBoundingClientRect();
    }
    const point = {x:rect.left + rect.width / 2, y:rect.top + rect.height / 2};
    const hit = document.elementFromPoint(point.x, point.y);
    if (!hit || (hit !== best && !best.contains(hit))) return {
        selector:'', descriptor:'', running, stop_visible:running,
        send_candidate_count:sendCandidates.length
    };
    best.setAttribute(markerName, markerValue);
    const descriptor = best.id ? `#${best.id}`
        : best.getAttribute('data-testid') ? `[data-testid=${best.getAttribute('data-testid')}]`
        : best.getAttribute('aria-label') ? `[aria-label=${best.getAttribute('aria-label')}]`
        : best.getAttribute('type') === 'submit' ? 'button[type=submit]' : 'composer:send-control';
    return {
        selector: `[${markerName}="${markerValue}"]`, descriptor, running,
        stop_visible: running, send_candidate_count: sendCandidates.length, point
    };
}"#;

const REQUEST_SUBMIT_SCRIPT: &str = r#"() => {
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
    if (buttons.some(node => /\bstop\b|停止|中止|cancel generation|interrupt/.test(label(node)))) {
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

const INSPECT_SUBMISSION_SCRIPT: &str = r#"payload => {
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
    const read = node => normalize(('value' in node) ? node.value : (node.innerText || node.textContent || ''));
    const marked = document.querySelector('[data-cccc-web-model-composer="cccc-web-model-composer"]');
    const markedText = marked ? read(marked) : '';
    const composers = Array.from(new Set([
        ...(marked ? [marked] : []),
        ...document.querySelectorAll('main textarea, main [role="textbox"], main [contenteditable="true"]'),
        ...document.querySelectorAll('textarea, [role="textbox"], [contenteditable="true"]')
    ])).filter(editable);
    const composerTexts = composers.map(read).filter(Boolean);
    // Virtualized history can grow without a send, and assistants may quote a
    // pending batch. Only the exact receipt inside a user message proves it.
    const messageNodes = Array.from(document.querySelectorAll('[data-message-author-role="user"], [data-testid="bot-transcript-scroller"] [data-testid="user-message"][role="article"]'));
    const needles = Array.isArray(payload.needles) ? payload.needles.map(normalize).filter(Boolean) : [];
    const echoFound = messageNodes.some(node => {
        const text = read(node);
        return needles.some(needle => {
            let at = text.indexOf(needle);
            while (at !== -1) {
                const end = at + needle.length;
                if ((at === 0 || /\s/.test(text[at - 1])) && (end === text.length || /\s/.test(text[end]))) return true;
                at = text.indexOf(needle, at + 1);
            }
            return false;
        });
    });
    const controls = Array.from(document.querySelectorAll('button, [role="button"]')).filter(visible);
    const label = node => [node.getAttribute('aria-label') || '', node.getAttribute('title') || '',
        node.getAttribute('data-testid') || '', node.id || '', node.innerText || node.textContent || '']
        .join(' ').replace(/\s+/g, ' ').trim().toLowerCase();
    const working = document.querySelector('[data-testid="bot-working-slot"]');
    const stopVisible = !!working && Array.from(working.children).some(visible) || controls.some(node => /\bstop\b|停止|中止|cancel generation|interrupt/.test(label(node)));
    const safeSend = controls.filter(node => {
        const text = label(node);
        if (/\bstop\b|停止|中止|cancel|retry|signin|sign in|log in|login|voice|microphone|attach|upload/.test(text)) return false;
        return node.id === 'composer-submit-button'
            || /(^|[-_])send([-_]|$)/.test(String(node.getAttribute('data-testid') || '').toLowerCase())
            || node.getAttribute('type') === 'submit' || /\bsend\b|\bsubmit\b|发送|送信/.test(text);
    });
    return {
        url: location.href || '', echo_found: echoFound, running: stopVisible, stop_visible: stopVisible,
        composer_exact: Boolean(markedText && markedText === expected),
        composer_contains_prompt: composerTexts.some(containsPrompt), composer_chars: markedText.length,
        user_message_count: messageNodes.length,
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

pub(super) async fn composer_has_attachments(page: &Page) -> Result<bool> {
    // An empty text box can still contain a user's pending file upload.
    // Restrict previews to the composer, never earlier chat images.
    page.evaluate(r#"() => {
            if ([...document.querySelectorAll('input[type=file]')].some(e => e.files?.length)) return true;
            const composer = document.querySelector('#prompt-textarea, [data-cccc-web-model-composer]');
            let root = composer?.closest('form, [data-testid*="composer" i], [class*="composer" i]');
            if (!root && composer?.closest('[data-testid="chat-input"]')) {
                for (let parent = composer.parentElement, depth = 0; parent && depth < 6; parent = parent.parentElement, depth++) {
                    if (parent.querySelector('[data-testid="chat-submit"]')) { root = parent; break; }
                }
            }
            return !!root && [...root.querySelectorAll('[data-testid*="file-preview" i], [data-testid="attachment" i], [data-testid*="attachment-preview" i], [data-testid*="upload-preview" i], [class*="file-preview" i], [class*="attachment-preview" i], [class*="upload-preview" i], img[src^="blob:"]')]
                .some(e => e.getClientRects().length > 0);
        }"#).await?.into_value::<bool>().map_err(Into::into)
}

#[cfg(test)]
mod readiness_tests {
    use super::*;

    #[tokio::test]
    async fn image_compat_submission_waits_for_enabled_button_and_sends_once() {
        use base64::Engine as _;

        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let image = temp.path().join("compat.png");
        std::fs::write(&image, base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAAKUlEQVR42u3OIQEAAAACIP+f1hkWWEB6FgEBAQEBAQEBAQEBAQEBgXdgl/rw4tnPBf0AAAAASUVORK5CYII=").expect("PNG")).expect("image");
        let (url, server) = super::super::browser_surface_tests::local_page(r#"<main><form><textarea id="prompt-textarea"></textarea><input id="upload-photos" type="file" accept="image/png"><button id="composer-submit-button" type="submit" disabled>Send</button></form></main><script>
            window.sends=0;window.uploadReady=false;window.earlySend=false;
            const button=document.querySelector('button');
            document.querySelector('input').onchange=()=>{
                const image=document.createElement('div');image.dataset.testid='attachment';image.textContent='Uploading compat.png';document.querySelector('form').prepend(image);
                setTimeout(()=>{window.uploadReady=true;image.textContent='compat.png';button.disabled=false},600);
            };
            button.onclick=e=>{
                e.preventDefault();e.stopPropagation();window.sends++;window.earlySend=!window.uploadReady;
                const t=document.querySelector('textarea'),a=document.createElement('article');
                a.dataset.messageAuthorRole='user';a.textContent=t.value;document.querySelector('main').prepend(a);t.value='';
            };
        </script>"#).await;
        let manager = BrowserSurfaces::default();
        manager
            .open("upload", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let prompt = "[cccc] Browser batch webdelivery:A:image events=e1 actor=A\nTask";
        let result = manager
            .submit_prompt_with_attachment("upload", &url, prompt, Some(&image), "image", None)
            .await
            .expect("submit");
        let PromptSubmissionOutcome::Verified(evidence) = result else {
            panic!("upload must send through the ready button")
        };
        assert_eq!(
            evidence["send_selector"],
            "#composer-submit-button:dom_click"
        );
        let page = manager.page("upload").await.expect("page");
        assert_eq!(
            page.evaluate("({sends:window.sends,early:window.earlySend})")
                .await
                .expect("state")
                .into_value::<Value>()
                .expect("json"),
            json!({"sends":1,"early":false})
        );
        manager.close("upload").await.expect("close");
        server.abort();
    }

    #[tokio::test]
    async fn send_activation_rechecks_ownership_and_uses_the_button_handler_once() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let manager = BrowserSurfaces::default();
        let (url, server) = super::super::browser_surface_tests::local_page("<main></main>").await;
        manager
            .open("activation", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let page = manager.page("activation").await.expect("page");
        let observed_url = page.url().await.expect("URL").expect("loaded URL");
        let prompt = "[cccc] Browser batch webdelivery:A:activation events=e1 actor=A\nTask";
        let needles = submission_needles(prompt);
        for case in [
            "edited",
            "disabled",
            "aria_disabled",
            "covered",
            "running",
            "manual",
            "navigated",
            "replaced",
            "ready",
        ] {
            page.evaluate(format!(r#"(() => {{
                history.replaceState(null,'',{});
                document.body.innerHTML='<main><form><textarea id="prompt-textarea"></textarea><button id="composer-submit-button" type="submit" style="position:absolute;left:20px;top:200px;width:80px;height:30px">Send</button></form></main>';
                const input=document.querySelector('textarea'); input.value={};
                window.sends=0; window.formSubmits=0; window.pointerDowns=0;
                document.querySelector('button').onpointerdown=()=>window.pointerDowns++;
                document.querySelector('form').onsubmit=e=>{{e.preventDefault();window.formSubmits++}};
                document.querySelector('button').onclick=e=>{{
                    e.preventDefault(); e.stopPropagation(); window.sends++;
                    const a=document.createElement('article'); a.dataset.messageAuthorRole='user';
                    a.textContent=input.value; document.querySelector('main').prepend(a); input.value='';
                }};
            }})()"#, json!(url), json!(prompt))).await.expect("fixture");
            wait_for_composer(&page).await.expect("composer");
            let candidate = inspect_send_control(&page).await.expect("ready control");
            assert!(!candidate.selector.is_empty());
            let mutation = match case {
                "edited" => "document.querySelector('textarea').value='Human draft'",
                "disabled" => "document.querySelector('button').disabled=true",
                "aria_disabled" => {
                    "document.querySelector('button').setAttribute('aria-disabled','true')"
                }
                "covered" => {
                    "document.body.insertAdjacentHTML('beforeend','<div style=\"position:absolute;left:20px;top:200px;width:80px;height:30px;background:white\"></div>')"
                }
                "running" => {
                    "document.body.insertAdjacentHTML('beforeend','<button aria-label=Stop>Stop</button>')"
                }
                "manual" => {
                    "const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=document.querySelector('textarea').value;document.querySelector('main').prepend(a)"
                }
                "navigated" => "history.replaceState(null,'','/another-conversation')",
                "replaced" => "document.querySelector('button').id='another-control'",
                _ => "void 0",
            };
            page.evaluate(mutation).await.expect("intervening change");
            let activation =
                activate_send_control(&page, &candidate, prompt, &needles, &observed_url)
                    .await
                    .expect("activation");
            assert_eq!(activation.invoked, case == "ready", "{case}");
            assert_eq!(activation.unsafe_state, case != "ready", "{case}");
            assert!(activation.error.is_empty());
            let actual = page.evaluate("({sends:window.sends,formSubmits:window.formSubmits,pointerDowns:window.pointerDowns,draft:document.querySelector('textarea').value})").await.expect("result").into_value::<Value>().expect("json");
            assert_eq!(actual["sends"], u32::from(case == "ready"), "{case}");
            assert_eq!(actual["formSubmits"], 0, "do not bypass the button handler");
            assert_eq!(actual["pointerDowns"], 0, "no native pointer dispatch");
            assert_eq!(
                actual["draft"],
                match case {
                    "ready" => "",
                    "edited" => "Human draft",
                    _ => prompt,
                },
                "{case}"
            );
            if case == "ready" {
                assert_eq!(activation.action, "#composer-submit-button:dom_click");
                let repeated =
                    activate_send_control(&page, &candidate, prompt, &needles, &observed_url)
                        .await
                        .expect("receipt check");
                assert!(
                    !repeated.invoked,
                    "an existing receipt prevents resubmission"
                );
            }
        }
        manager.close("activation").await.expect("close");
        server.abort();
    }

    #[tokio::test]
    async fn submission_receipt_requires_the_current_batch_in_a_user_message() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let manager = BrowserSurfaces::default();
        let (url, server) = super::super::browser_surface_tests::local_page(
            "<main><textarea id='prompt-textarea'></textarea></main>",
        )
        .await;
        manager
            .open("receipt", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let page = manager.page("receipt").await.expect("page");
        let prompt =
            "[cccc] Browser batch webdelivery:A:current events=e1,e2 actor=A\nDo the task.";
        let _ = wait_for_composer(&page).await.expect("composer");
        for (role, text) in [
            ("assistant", prompt),
            (
                "user",
                "[cccc] Browser batch webdelivery:A:old events=e1,e2 actor=A",
            ),
            (
                "user",
                "[cccc] Browser batch webdelivery:A:current events=e1,e2 actor=B",
            ),
            (
                "user",
                "[cccc] Browser batch webdelivery:A:current events=e1,e2 actor=AB",
            ),
        ] {
            page.evaluate(format!("(() => {{const a=document.createElement('article');a.dataset.messageAuthorRole={};a.dataset.testid='conversation-turn-1';a.textContent={};document.querySelector('main').append(a);}})()",json!(role),json!(text))).await.expect("non-receipt");
            let snapshot = inspect_submission(&page, prompt, &submission_needles(prompt))
                .await
                .expect("inspect");
            assert!(!snapshot.echo_found, "{role}: {text}");
        }
        page.evaluate(format!("(() => {{const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent={};document.querySelector('main').append(a);}})()",json!(prompt))).await.expect("actual receipt");
        assert!(
            inspect_submission(&page, prompt, &submission_needles(prompt))
                .await
                .expect("inspect receipt")
                .echo_found
        );
        manager.close("receipt").await.expect("close");
        server.abort();
    }

    #[tokio::test]
    async fn send_wait_preserves_covered_controls_and_changed_drafts() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let manager = BrowserSurfaces::default();
        let (url, server) = super::super::browser_surface_tests::local_page(r#"<main><form onsubmit="event.preventDefault();window.sends++"><textarea id="prompt-textarea"></textarea><button id="composer-submit-button" type="submit" style="position:absolute;left:20px;top:200px;width:80px;height:30px">Send</button></form><div id="cover" style="position:absolute;left:20px;top:200px;width:80px;height:30px;background:white" onclick="window.wrongClicks++"></div></main><script>window.sends=0;window.wrongClicks=0;</script>"#).await;
        manager
            .open("protected", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let page = manager.page("protected").await.expect("page");
        let result = manager
            .submit_prompt_with_attachment("protected", &url, "Pending batch", None, "first", None)
            .await
            .expect("covered send");
        assert!(
            matches!(result, PromptSubmissionOutcome::Deferred(_)),
            "a covered button cannot fall through to form or keyboard submission"
        );
        assert_eq!(page.evaluate("({sends:window.sends, wrong:window.wrongClicks,draft:document.querySelector('textarea').value})").await.expect("snapshot").into_value::<Value>().expect("json"),json!({"sends":0,"wrong":0,"draft":"Pending batch"}));
        page.evaluate("document.querySelector('#cover').remove();const t=document.querySelector('textarea');t.value='';t.oninput=()=>{const b=document.querySelector('button');b.disabled=true;setTimeout(()=>{t.value='Human draft edited during send readiness';b.disabled=false},100)}").await.expect("edit during wait");
        let result = manager
            .submit_prompt_with_attachment("protected", &url, "Next batch", None, "next", None)
            .await
            .expect("changed draft");
        assert!(matches!(result, PromptSubmissionOutcome::Ambiguous(_)));
        assert_eq!(
            page.evaluate("({sends:window.sends,draft:document.querySelector('textarea').value})")
                .await
                .expect("preserved draft")
                .into_value::<Value>()
                .expect("json"),
            json!({"sends":0,"draft":"Human draft edited during send readiness"})
        );
        // A manual send may appear before React clears the composer. Receipt
        // ownership, not the remaining draft, must prevent a second click.
        page.evaluate("(() => {const t=document.querySelector('textarea');t.value='';t.oninput=()=>{const b=document.querySelector('button');b.disabled=true;setTimeout(()=>{const a=document.createElement('article');a.dataset.messageAuthorRole='user';a.textContent=t.value;document.querySelector('main').append(a);b.disabled=false},100)};})()").await.expect("manual receipt during wait");
        let result = manager
            .submit_prompt_with_attachment(
                "protected",
                &url,
                "Manually sent batch",
                None,
                "manual",
                None,
            )
            .await
            .expect("manual receipt");
        assert!(matches!(result, PromptSubmissionOutcome::Verified(_)));
        assert_eq!(
            page.evaluate("window.sends")
                .await
                .expect("no duplicate send")
                .into_value::<u32>()
                .expect("count"),
            0
        );
        manager.close("protected").await.expect("close");
        server.abort();
    }

    #[tokio::test]
    async fn owned_draft_recovery_rechecks_composer_and_page_before_replacement() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let manager = BrowserSurfaces::default();
        let (url, server) = super::super::browser_surface_tests::local_page("<main></main>").await;
        manager
            .open("owned", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("browser");
        let page = manager.page("owned").await.expect("page");
        let old = "[user -> browser-test] Verified old draft.";
        let prompt = "[CCCC] Rebuilt bootstrap and recovered delivery batch.";
        for editable in [false, true] {
            for case in [
                "unchanged",
                "edited",
                "cleared",
                "history",
                "generating",
                "navigated",
            ] {
                let element = if editable {
                    "<div id='prompt-textarea' contenteditable='true' role='textbox' style='width:400px;height:100px'></div>"
                } else {
                    "<textarea id='prompt-textarea' style='width:400px;height:100px'></textarea>"
                };
                let html = format!(
                    "<main><form>{element}<button type='submit'>Send</button></form></main>"
                );
                page.evaluate(format!(r#"(() => {{
                    history.replaceState(null,'',{});
                    document.body.innerHTML={};
                    window.sends=0;
                    window.readDraft=()=>{{ const n=document.querySelector('#prompt-textarea'); return 'value' in n?n.value:n.textContent; }};
                    window.writeDraft=(text)=>{{ const n=document.querySelector('#prompt-textarea'); if('value' in n)n.value=text;else n.textContent=text; }};
                    window.writeDraft({});
                    document.querySelector('form').onsubmit=(e)=>{{e.preventDefault(); window.sends++; const a=document.createElement('article'); a.dataset.messageAuthorRole='user'; a.textContent=window.readDraft(); document.querySelector('main').append(a); window.writeDraft('');}};
                }})()"#, serde_json::to_string(&url).expect("url"),serde_json::to_string(&html).expect("html"),serde_json::to_string(old).expect("old"))).await.expect("reset fixture");
                assert_eq!(
                    manager
                        .inspect_staged_prompt("owned", &url, old)
                        .await
                        .expect("precheck")["recoverable"],
                    true
                );
                let mutation = match case {
                    "edited" => "window.writeDraft('A newer human draft')",
                    "cleared" => "window.writeDraft('')",
                    "history" => {
                        "document.querySelector('main').insertAdjacentHTML('beforeend', '<article data-message-author-role=user>A message already sent</article>')"
                    }
                    "generating" => {
                        "document.querySelector('main').insertAdjacentHTML('beforeend', '<button aria-label=Stop>Stop</button>')"
                    }
                    "navigated" => "history.replaceState(null,'','/another-page')",
                    _ => "void 0",
                };
                page.evaluate(mutation)
                    .await
                    .expect("change after early inspection");
                let before: String = page
                    .evaluate("window.readDraft()")
                    .await
                    .expect("before")
                    .into_value()
                    .expect("text");
                let page_before = page.url().await.expect("url");
                let outcome = manager
                    .submit_prompt_with_attachment(
                        "owned",
                        &url,
                        prompt,
                        None,
                        "recovered",
                        Some(old),
                    )
                    .await
                    .expect("recovery");
                let sends: usize = page
                    .evaluate("window.sends")
                    .await
                    .expect("sends")
                    .into_value()
                    .expect("count");
                if case == "unchanged" {
                    assert!(
                        matches!(outcome, PromptSubmissionOutcome::Verified(_)),
                        "owned draft must recover"
                    );
                    assert_eq!(sends, 1);
                } else {
                    let PromptSubmissionOutcome::Ambiguous(evidence) = outcome else {
                        panic!("changed recovery must stop: {case}")
                    };
                    assert_eq!(
                        evidence["submission_evidence"], "staged_draft_changed",
                        "{case}"
                    );
                    assert_eq!(sends, 0, "{case}");
                    assert_eq!(
                        page.evaluate("window.readDraft()")
                            .await
                            .expect("after")
                            .into_value::<String>()
                            .expect("text"),
                        before,
                        "preserve {case}"
                    );
                    assert_eq!(
                        page.url().await.expect("url"),
                        page_before,
                        "do not navigate on {case}"
                    );
                }
            }
        }
        manager.close("owned").await.expect("close");
        server.abort();
    }

    #[tokio::test]
    async fn unverified_submission_and_user_drafts_are_never_overwritten() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let manager = BrowserSurfaces::default();
        let (url, server) = super::super::browser_surface_tests::local_page(
            "<main><article data-message-author-role='user'>Existing message</article><form onsubmit='event.preventDefault(); window.clicks=(window.clicks||0)+1'><textarea id='prompt-textarea'></textarea><button type='submit'>Send</button></form></main>"
        ).await;
        manager
            .open("draft", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let page = manager.page("draft").await.expect("page");
        let first = manager
            .submit_prompt_with_attachment(
                "draft",
                &url,
                "original unsent batch",
                None,
                "first",
                None,
            )
            .await
            .expect("attempt");
        assert!(matches!(first, PromptSubmissionOutcome::Ambiguous(_)));
        for (element, original) in [
            ("textarea", "original unsent batch"),
            ("contenteditable", "human draft"),
        ] {
            if element == "contenteditable" {
                page.evaluate("document.querySelector('textarea').outerHTML = '<div id=prompt-textarea contenteditable=true role=textbox style=width:400px;height:60px>human draft</div>'").await.expect("editable draft");
            }
            let next = manager
                .submit_prompt_with_attachment(
                    "draft",
                    &url,
                    "later incoming batch",
                    None,
                    "next",
                    None,
                )
                .await
                .expect("blocked");
            let PromptSubmissionOutcome::Ambiguous(evidence) = next else {
                panic!("must block occupied composer")
            };
            assert_eq!(evidence["submission_evidence"], "composer_occupied");
            let snapshot = inspect_submission(&page, original, &[])
                .await
                .expect("preserved");
            assert!(snapshot.composer_exact);
            assert_eq!(
                page.evaluate("window.clicks")
                    .await
                    .expect("clicks")
                    .into_value::<usize>()
                    .expect("number"),
                1
            );
        }
        page.evaluate("document.querySelector('#prompt-textarea').textContent=''; document.querySelector('form').onsubmit=(e)=>{e.preventDefault(); const input=document.querySelector('#prompt-textarea'); const msg=document.createElement('article'); msg.dataset.messageAuthorRole='user'; msg.textContent=input.textContent; document.querySelector('main').append(msg); input.textContent='';}").await.expect("manual clear");
        let next = manager
            .submit_prompt_with_attachment(
                "draft",
                &url,
                "confirmed new batch",
                None,
                "confirmed",
                None,
            )
            .await
            .expect("submit");
        assert!(matches!(next, PromptSubmissionOutcome::Verified(_)));
        manager.close("draft").await.expect("close");
        server.abort();
    }

    #[tokio::test]
    async fn chatgpt_auth_and_landing_inputs_are_not_conversation_composers() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = BrowserSurfaces::default();
        let (url, server) = super::super::browser_surface_tests::local_page("<main></main>").await;
        manager
            .open("composer", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let page = manager.page("composer").await.expect("page");
        page.evaluate("document.body.innerHTML = '<input name=username><textarea placeholder=Ask></textarea>'").await.expect("landing page");
        for host in ["chatgpt.com", "auth.openai.com"] {
            // Exercise the production selector against an offline DOM, supplying
            // only the hostname instead of contacting the provider in this test.
            let script =
                format!("(location => ({SELECT_COMPOSER_SCRIPT})())({{hostname:{host:?}}})");
            let candidate = page
                .evaluate(script)
                .await
                .expect("inspect inputs")
                .into_value::<ComposerCandidate>()
                .expect("candidate");
            assert!(
                candidate.selector.is_empty(),
                "{host} input is not a composer"
            );
        }
        page.evaluate("document.querySelector('textarea').id = 'prompt-textarea'")
            .await
            .expect("conversation");
        let candidate = page
            .evaluate(format!(
                "(location => ({SELECT_COMPOSER_SCRIPT})())({{hostname:'chatgpt.com'}})"
            ))
            .await
            .expect("composer")
            .into_value::<ComposerCandidate>()
            .expect("candidate");
        assert!(!candidate.selector.is_empty());
        manager.close("composer").await.expect("close");
        server.abort();
    }
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod grok_tests;
