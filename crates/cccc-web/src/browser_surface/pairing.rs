//! A pairing receipt must return to the same owned Page that received the code.
//! No navigation, active-tab lookup, browser recovery, or content conversion.
use super::BrowserSurfaces;
use anyhow::{Context, Result, bail};
use chromiumoxide::Page;
use serde_json::{Value, json};

pub(crate) struct PairingPage {
    page: Page,
    initial_url: String,
    conversation: Option<String>,
}

impl BrowserSurfaces {
    pub(crate) async fn pairing_page(&self, key: &str) -> Result<PairingPage> {
        let page = self
            .sessions
            .lock()
            .await
            .get(key)
            .map(|s| s.page.clone())
            .context("pairing_browser_required")?;
        let initial_url = page.url().await?.unwrap_or_default();
        let conversation = cccc_core::web_model_connectors::conversation_url(&initial_url).ok();
        if conversation.is_none() && !new_chat_url(&initial_url) {
            bail!("pairing_browser_required");
        }
        let target = PairingPage {
            page,
            initial_url,
            conversation,
        };
        let readiness = self.prompt_readiness(key).await?;
        if readiness["composer_chars"].as_u64().unwrap_or(0) > 0 {
            bail!("pairing_composer_occupied");
        }
        if readiness["ready"] != true || readiness["running"] == true {
            bail!("pairing_browser_not_ready");
        }
        target.check_before_send(self, key).await?;
        Ok(target)
    }
}

impl PairingPage {
    pub(crate) async fn check_before_send(
        &self,
        surfaces: &BrowserSurfaces,
        key: &str,
    ) -> Result<()> {
        let url = self.current_url(surfaces, key).await?;
        if url != self.initial_url {
            bail!("pairing_target_changed");
        }
        let attachments = super::prompt_submission::composer_has_attachments(&self.page).await?;
        if attachments {
            bail!("pairing_composer_occupied");
        }
        Ok(())
    }

    async fn current_url(&self, surfaces: &BrowserSurfaces, key: &str) -> Result<String> {
        let same_target = surfaces
            .sessions
            .lock()
            .await
            .get(key)
            .is_some_and(|s| s.page.target_id() == self.page.target_id());
        if !same_target {
            bail!("pairing_target_changed");
        }
        self.page.url().await?.context("pairing_interrupted")
    }

    pub(crate) async fn receipt_url(
        &self,
        surfaces: &BrowserSurfaces,
        key: &str,
        prompt: &str,
        receipt: Option<&str>,
    ) -> Result<Option<String>> {
        let url = self.current_url(surfaces, key).await?;
        let conversation = cccc_core::web_model_connectors::conversation_url(&url).ok();
        if self
            .conversation
            .as_ref()
            .is_some_and(|expected| conversation.as_ref() != Some(expected))
            || (conversation.is_none() && !pending_new_chat_url(&url))
        {
            bail!("pairing_target_changed");
        }
        // ChatGPT first assigns /c/WEB:<client-id>, then a server conversation
        // URL. A handshake started on the new-chat page waits through that
        // transition, even if the receipt arrives first. Never bind this URL.
        // An existing conversation still fails the identity check above.
        if conversation.is_none() {
            return Ok(None);
        }
        let Some(receipt) = receipt.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let result = self
            .page
            .evaluate(super::chatgpt_dom::with_messages(
                RECEIPT_SCRIPT,
                &json!({"prompt":prompt,"receipt":receipt}).to_string(),
            ))
            .await?
            .into_value::<Value>()?;
        // Read location and DOM in one evaluation. Also reject a navigation
        // during inspection instead of associating an old DOM with a new URL.
        if result["url"].as_str() != Some(url.as_str())
            || self.current_url(surfaces, key).await? != url
        {
            bail!("pairing_target_changed");
        }
        Ok(if result["verified"] == true {
            conversation
        } else {
            None
        })
    }
}

fn new_chat_url(raw: &str) -> bool {
    chatgpt_page_url(raw).is_some_and(|url| url.path() == "/")
}

fn pending_new_chat_url(raw: &str) -> bool {
    chatgpt_page_url(raw).is_some_and(|url| {
        url.path() == "/"
            || url.path().strip_prefix("/c/").is_some_and(|id| {
                !id.contains('/') && super::prompt_submission::provisional_conversation_id(id)
            })
    })
}

fn chatgpt_page_url(raw: &str) -> Option<reqwest::Url> {
    reqwest::Url::parse(raw).ok().filter(|url| {
        url.scheme() == "https"
            && matches!(url.host_str(), Some("chatgpt.com" | "chat.openai.com"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
    })
}

const RECEIPT_SCRIPT: &str = r#"({prompt,receipt}, messages) => {
  const normalize = s => String(s || '').replace(/\s+/g, ' ').trim();
  const index = messages.findLastIndex(e => e.role === 'user' && normalize(e.node.innerText) === normalize(prompt));
  let verified = false;
  if (index >= 0) for (const e of messages.slice(index + 1)) {
    if (e.role === 'user') break;
    if (e.node.getClientRects().length && (e.node.innerText || '').includes(receipt)) { verified = true; break; }
  }
  return {url:location.href,verified};
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_chatgpt_pairing_receipt_keeps_exact_user_and_assistant_boundaries() {
        if crate::system_browser_path().is_none() {
            return;
        }
        let _chrome_guard = super::super::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("fixture");
        let (url, server) = super::super::browser_surface_tests::local_page("<main></main>").await;
        let surfaces = BrowserSurfaces::default();
        surfaces
            .open("pairing-dom", &temp.path().join("profile"), &url, 800, 600)
            .await
            .expect("open");
        let page = surfaces
            .sessions
            .lock()
            .await
            .get("pairing-dom")
            .expect("session")
            .page
            .clone();
        let user = |text: &str| {
            format!(
                "<div data-content-search-unit-key><div data-user-message-bubble>{text}</div></div>"
            )
        };
        let assistant = |text: &str| {
            format!(
                "<div data-content-search-unit-key><h4 data-conversation-role=assistant>Assistant</h4><div>{text}</div></div>"
            )
        };
        let prompt = "Pair fixture-code with fixture-receipt";
        let receipt = "fixture-receipt";
        for (label, html, verified) in [
            (
                "returned receipt",
                format!("{}{}", user(prompt), assistant(receipt)),
                true,
            ),
            (
                "user quote is not a reply",
                format!("{}{}", user(prompt), assistant("No receipt")),
                false,
            ),
            (
                "earlier reply",
                format!("{}{}", assistant(receipt), user(prompt)),
                false,
            ),
            (
                "later user boundary",
                format!(
                    "{}{}{}",
                    user(prompt),
                    user("Different task"),
                    assistant(receipt)
                ),
                false,
            ),
            (
                "wrong setup code",
                format!("{}{}", user("Pair an old code"), assistant(receipt)),
                false,
            ),
            (
                "mixed DOM",
                format!(
                    "<article data-message-author-role=user>{}</article><article data-message-author-role=assistant>{}</article>",
                    user(prompt),
                    assistant(receipt)
                ),
                true,
            ),
        ] {
            page.evaluate(format!(
                "document.querySelector('main').innerHTML={}",
                json!(html)
            ))
            .await
            .expect("fixture DOM");
            let result = page
                .evaluate(super::super::chatgpt_dom::with_messages(
                    RECEIPT_SCRIPT,
                    &json!({"prompt":prompt,"receipt":receipt}).to_string(),
                ))
                .await
                .expect("receipt check")
                .into_value::<Value>()
                .expect("result");
            assert_eq!(result["verified"], verified, "{label}");
        }
        surfaces.close("pairing-dom").await.expect("close");
        server.abort();
    }

    #[test]
    fn only_native_new_chat_routes_can_remain_pending() {
        for url in [
            "https://chatgpt.com/",
            "https://chatgpt.com/?model=fixture",
            "https://chatgpt.com/c/WEB:00000000-0000-4000-8000-000000000001",
            "https://chatgpt.com/c/WEB%3A00000000-0000-4000-8000-000000000001",
            "https://chat.openai.com/c/WEB:pending",
        ] {
            assert!(pending_new_chat_url(url), "{url}");
            assert!(
                cccc_core::web_model_connectors::conversation_url(url).is_err(),
                "pending URL must not grant authority: {url}"
            );
        }
        for url in [
            "https://chatgpt.com/c/stable-id",
            "https://chatgpt.com/c/WEB:pending/other",
            "https://chatgpt.com/auth/login",
            "http://chatgpt.com/c/WEB:pending",
            "https://chatgpt.com.example/c/WEB:pending",
            "https://other.chatgpt.com/c/WEB:pending",
            "https://user@chatgpt.com/c/WEB:pending",
            "https://chatgpt.com:8443/c/WEB:pending",
            "about:blank",
        ] {
            assert!(!pending_new_chat_url(url), "{url}");
        }
    }
}
