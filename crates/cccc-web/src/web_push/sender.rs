//! One encrypted POST per notification and subscription (RFC 8030). Logs
//! name the push service and the status, never the endpoint or the keys:
//! an endpoint is the capability to notify that browser.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE};
use url::Url;

use super::crypto::{CryptoError, ReceiverKeys, VapidKey, encrypt};
use super::endpoint;
use super::store::Subscription;

/// How long a push service keeps a notification for a device that is off.
const TTL_SECONDS: &str = "86400";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// RFC 8292 allows at most 24 hours; a token is reused until an hour is left.
const TOKEN_LIFETIME: u64 = 12 * 60 * 60;
const TOKEN_RENEW_BEFORE: u64 = 60 * 60;
/// The contact RFC 8292 asks for, and Apple requires; it names the software
/// that sends, not the person who uses it.
const SUBJECT: &str = "https://github.com/ChesterRa/cccc";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Accepted,
    /// The push service no longer knows the subscription (404 or 410), or
    /// it was stored in a form that can never be sent to.
    Gone,
    /// Worth another try later: rate limited, a service error, or no answer.
    Retry,
    /// Refused for a reason another try would not change.
    Refused,
}

pub(crate) struct Sender {
    client: reqwest::Client,
    /// Authorization values per push service origin, with their expiry.
    tokens: Mutex<HashMap<String, (String, u64)>>,
    /// Tests send each endpoint to a push service on loopback.
    #[cfg(test)]
    pub(crate) stand_ins: HashMap<String, Url>,
}

impl Sender {
    pub(crate) fn new() -> Result<Self, reqwest::Error> {
        Self::with_timeouts(REQUEST_TIMEOUT, CONNECT_TIMEOUT)
    }

    pub(crate) fn with_timeouts(
        request: Duration,
        connect: Duration,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(request)
            .connect_timeout(connect)
            .build()?;
        Ok(Self {
            client,
            tokens: Mutex::new(HashMap::new()),
            #[cfg(test)]
            stand_ins: HashMap::new(),
        })
    }

    pub(crate) async fn send(
        &self,
        key: &VapidKey,
        subscription: &Subscription,
        payload: &[u8],
    ) -> Outcome {
        let Some(url) = endpoint::validate(&subscription.endpoint) else {
            return Outcome::Gone;
        };
        let keys = match ReceiverKeys::parse(&subscription.p256dh, &subscription.auth) {
            Ok(keys) => keys,
            // Our own failure says nothing about the subscription.
            Err(CryptoError::Internal) => return Outcome::Retry,
            Err(_) => return Outcome::Gone,
        };
        let body = match encrypt(payload, &keys) {
            Ok(body) => body,
            Err(CryptoError::InvalidKey) => return Outcome::Gone,
            Err(CryptoError::TooLarge) => return Outcome::Refused,
            Err(CryptoError::Internal) => return Outcome::Retry,
        };
        let audience = endpoint::audience(&url);
        // A token that cannot be signed now, as with a clock set before
        // 1970, may be signed on a later try.
        let Some(authorization) = self.authorization(key, &audience) else {
            return Outcome::Retry;
        };
        // Tests reach only the push services they stand up on loopback.
        #[cfg(test)]
        let Some(url) = self.stand_ins.get(&subscription.endpoint).cloned() else {
            return Outcome::Retry;
        };
        match self
            .post_status(url.clone(), authorization, body.clone())
            .await
        {
            // A rejected token is not reused: one signed while the clock was
            // wrong would stay cached for hours. A fresh one is tried once.
            Some(401 | 403) => {
                self.forget_token(&audience);
                let Some(fresh) = self.authorization(key, &audience) else {
                    return Outcome::Retry;
                };
                self.post(url, fresh, body).await
            }
            status => outcome(&url, status),
        }
    }

    async fn post(&self, url: Url, authorization: String, body: Vec<u8>) -> Outcome {
        let status = self.post_status(url.clone(), authorization, body).await;
        outcome(&url, status)
    }

    /// The status a push service answered, or `None` when it did not.
    async fn post_status(&self, url: Url, authorization: String, body: Vec<u8>) -> Option<u16> {
        let service = url.host_str().unwrap_or_default().to_owned();
        let response = self
            .client
            .post(url)
            .header("TTL", TTL_SECONDS)
            .header("Urgency", "normal")
            .header(CONTENT_ENCODING, "aes128gcm")
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(AUTHORIZATION, authorization)
            .body(body)
            .send()
            .await;
        match response {
            Ok(response) => Some(response.status().as_u16()),
            Err(error) => {
                tracing::warn!(
                    service,
                    timeout = error.is_timeout(),
                    connect = error.is_connect(),
                    "Web Push service did not answer"
                );
                None
            }
        }
    }

    fn forget_token(&self, audience: &str) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.remove(audience);
        }
    }

    fn authorization(&self, key: &VapidKey, audience: &str) -> Option<String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let mut tokens = self.tokens.lock().ok()?;
        tokens.retain(|_, (_, expires_at)| *expires_at > now + TOKEN_RENEW_BEFORE);
        if let Some((value, _)) = tokens.get(audience)
            && value.ends_with(key.public_key())
        {
            return Some(value.clone());
        }
        let expires_at = now + TOKEN_LIFETIME;
        let value = key.authorization(audience, SUBJECT, expires_at).ok()?;
        tokens.insert(audience.to_owned(), (value.clone(), expires_at));
        Some(value)
    }
}

/// What an answer, or none, means; anything but acceptance is logged with
/// the push service and the status only.
fn outcome(url: &Url, status: Option<u16>) -> Outcome {
    let Some(status) = status else {
        return Outcome::Retry;
    };
    let outcome = classify(status);
    if outcome != Outcome::Accepted {
        tracing::warn!(
            service = url.host_str().unwrap_or_default(),
            status,
            "Web Push service refused a notification"
        );
    }
    outcome
}

fn classify(status: u16) -> Outcome {
    match status {
        200..=299 => Outcome::Accepted,
        404 | 410 => Outcome::Gone,
        408 | 429 | 500..=599 => Outcome::Retry,
        _ => Outcome::Refused,
    }
}

#[cfg(test)]
mod tests;
