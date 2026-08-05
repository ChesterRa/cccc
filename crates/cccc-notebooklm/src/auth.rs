use regex::Regex;
use reqwest::header::COOKIE;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

#[derive(Debug, Deserialize)]
struct StorageState {
    #[serde(default)]
    cookies: Vec<Cookie>,
    #[serde(default)]
    authuser: usize,
}

#[derive(Debug, Deserialize)]
struct Cookie {
    name: String,
    value: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    expires: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthState {
    pub(crate) csrf_token: String,
    pub(crate) session_id: String,
    pub(crate) authuser: usize,
}

pub(crate) fn parse_storage(raw: &str) -> Result<(serde_json::Value, String, usize)> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| Error::InvalidCredential(error.to_string()))?;
    let state: StorageState = serde_json::from_value(value.clone())
        .map_err(|error| Error::InvalidCredential(error.to_string()))?;
    let cookies = state
        .cookies
        .into_iter()
        .filter(|cookie| domain_matches("notebooklm.google.com", &cookie.domain))
        .filter(|cookie| !is_expired(cookie.expires))
        .filter(|cookie| !cookie.name.is_empty())
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ");
    if cookies.is_empty() {
        return Err(Error::InvalidCredential(
            "Playwright storage state must contain Google cookies".into(),
        ));
    }
    Ok((value, cookies, state.authuser))
}

fn is_expired(expires: Option<f64>) -> bool {
    let Some(expires) = expires.filter(|value| *value >= 0.0) else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64());
    expires <= now
}

fn domain_matches(host: &str, cookie_domain: &str) -> bool {
    let domain = cookie_domain.trim_start_matches('.');
    domain.is_empty()
        || host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

pub(crate) fn attach_cookie(
    request: reqwest::blocking::RequestBuilder,
    cookie_header: &str,
) -> reqwest::blocking::RequestBuilder {
    request.header(COOKIE, cookie_header)
}

pub(crate) fn extract_tokens(html: &str) -> Result<(String, String)> {
    Ok((extract_wiz(html, "SNlM0e")?, extract_wiz(html, "FdrFJe")?))
}

fn extract_wiz(html: &str, key: &'static str) -> Result<String> {
    let escaped = regex::escape(key);
    let patterns = [
        format!(r#"\"{escaped}\"\s*:\s*\"([^\"\\]*(?:\\.[^\"\\]*)*)\""#),
        format!(r#"'{escaped}'\s*:\s*'([^'\\]*(?:\\.[^'\\]*)*)'"#),
        format!(r#"&quot;{escaped}&quot;\s*:\s*&quot;(.*?)&quot;"#),
    ];
    for pattern in patterns {
        let Ok(regex) = Regex::new(&pattern) else {
            continue;
        };
        if let Some(value) = regex.captures(html).and_then(|capture| capture.get(1)) {
            let raw = value.as_str().replace("&amp;", "&").replace("&quot;", "\"");
            let quoted = format!("\"{raw}\"");
            return serde_json::from_str(&quoted).or(Ok(raw));
        }
    }
    Err(Error::InvalidCredential(format!(
        "authenticated NotebookLM page did not contain {key}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_storage_state_and_filters_unrelated_cookies() {
        let (_, header, authuser) = parse_storage(
            r#"{"authuser":2,"cookies":[{"name":"SID","value":"a","domain":".google.com"},{"name":"x","value":"b","domain":"example.com"}]}"#,
        )
        .expect("credential");
        assert_eq!(header, "SID=a");
        assert_eq!(authuser, 2);
    }

    #[test]
    fn excludes_google_sibling_domain_cookies() {
        let (_, header, _) = parse_storage(
            r#"{"cookies":[{"name":"SID","value":"ok","domain":".google.com"},{"name":"ACCOUNT","value":"private","domain":"accounts.google.com"}]}"#,
        )
        .expect("credential");
        assert_eq!(header, "SID=ok");
    }

    #[test]
    fn excludes_expired_cookies() {
        let (_, header, _) = parse_storage(
            r#"{"cookies":[{"name":"OLD","value":"expired","domain":".google.com","expires":1},{"name":"SID","value":"session","domain":".google.com","expires":-1}]}"#,
        )
        .expect("credential");
        assert_eq!(header, "SID=session");
    }

    #[test]
    fn extracts_wiz_tokens_from_supported_forms() {
        assert_eq!(
            extract_wiz(r#"{"SNlM0e":"csrf\"x"}"#, "SNlM0e").expect("token"),
            "csrf\"x"
        );
        assert_eq!(
            extract_wiz("'FdrFJe':'session'", "FdrFJe").expect("token"),
            "session"
        );
        assert_eq!(
            extract_wiz("&quot;SNlM0e&quot;:&quot;a&amp;b&quot;", "SNlM0e").expect("token"),
            "a&b"
        );
    }
}
