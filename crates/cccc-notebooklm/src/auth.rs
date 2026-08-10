use regex::Regex;
use reqwest::header::COOKIE;
use serde::Deserialize;
use std::cmp::Reverse;
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
    path: String,
    #[serde(default)]
    expires: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthState {
    pub(crate) csrf_token: String,
    pub(crate) session_id: String,
    pub(crate) authuser: usize,
}

pub(crate) fn parse_storage(
    raw: &str,
    request_host: &str,
    request_path: &str,
) -> Result<(serde_json::Value, String, usize)> {
    let (value, state) = decode_storage(raw)?;
    let authuser = state.authuser;
    let cookies = cookie_header(&state, request_host, request_path).ok_or_else(|| {
        Error::InvalidCredential("Playwright storage state must contain Google cookies".into())
    })?;
    Ok((value, cookies, authuser))
}

pub(crate) fn optional_cookie_header(
    raw: &str,
    request_host: &str,
    request_path: &str,
) -> Result<Option<String>> {
    let (_, state) = decode_storage(raw)?;
    Ok(cookie_header(&state, request_host, request_path))
}

fn decode_storage(raw: &str) -> Result<(serde_json::Value, StorageState)> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| Error::InvalidCredential(error.to_string()))?;
    let state: StorageState = serde_json::from_value(value.clone())
        .map_err(|error| Error::InvalidCredential(error.to_string()))?;
    Ok((value, state))
}

fn cookie_header(state: &StorageState, request_host: &str, request_path: &str) -> Option<String> {
    let mut cookies = state
        .cookies
        .iter()
        .filter(|cookie| domain_matches(request_host, &cookie.domain))
        .filter(|cookie| path_matches(request_path, &cookie.path))
        .filter(|cookie| !is_expired(cookie.expires))
        .filter(|cookie| !cookie.name.is_empty())
        .collect::<Vec<_>>();
    cookies.sort_by_key(|cookie| Reverse(normalized_cookie_path(&cookie.path).len()));
    let header = cookies
        .into_iter()
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ");
    (!header.is_empty()).then_some(header)
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
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let raw_domain = cookie_domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let Some(domain) = raw_domain.strip_prefix('.') else {
        return !raw_domain.is_empty() && host == raw_domain;
    };
    !domain.is_empty()
        && (host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.')))
}

fn normalized_cookie_path(path: &str) -> &str {
    if path.starts_with('/') { path } else { "/" }
}

fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    let request_path = if request_path.starts_with('/') {
        request_path
    } else {
        "/"
    };
    let cookie_path = normalized_cookie_path(cookie_path);
    request_path == cookie_path
        || request_path
            .strip_prefix(cookie_path)
            .is_some_and(|suffix| cookie_path.ends_with('/') || suffix.starts_with('/'))
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
            "notebook.google.com",
            "/",
        )
        .expect("credential");
        assert_eq!(header, "SID=a");
        assert_eq!(authuser, 2);
    }

    #[test]
    fn excludes_google_sibling_domain_cookies() {
        let (_, header, _) = parse_storage(
            r#"{"cookies":[{"name":"SID","value":"ok","domain":".google.com"},{"name":"ACCOUNT","value":"private","domain":"accounts.google.com"}]}"#,
            "notebook.google.com",
            "/",
        )
        .expect("credential");
        assert_eq!(header, "SID=ok");
    }

    #[test]
    fn excludes_expired_cookies() {
        let (_, header, _) = parse_storage(
            r#"{"cookies":[{"name":"OLD","value":"expired","domain":".google.com","expires":1},{"name":"SID","value":"session","domain":".google.com","expires":-1}]}"#,
            "notebook.google.com",
            "/",
        )
        .expect("credential");
        assert_eq!(header, "SID=session");
    }

    #[test]
    fn preserves_current_gemini_notebook_cookie_scope() {
        let raw = r#"{"cookies":[{"name":"SID","value":"global","domain":".google.com"},{"name":"OSID","value":"current","domain":"notebook.google.com"},{"name":"OSID","value":"legacy","domain":"notebooklm.google.com"}]}"#;
        let (_, current, _) =
            parse_storage(raw, "notebook.google.com", "/").expect("current credential");
        let (_, legacy, _) =
            parse_storage(raw, "notebooklm.google.com", "/").expect("legacy credential");
        assert_eq!(current, "SID=global; OSID=current");
        assert_eq!(legacy, "SID=global; OSID=legacy");
    }

    #[test]
    fn honors_host_only_domains_and_cookie_paths() {
        let raw = r#"{"cookies":[{"name":"GLOBAL","value":"g","domain":".google.com","path":"/"},{"name":"HOST_ONLY","value":"h","domain":"google.com","path":"/"},{"name":"ROOT","value":"r","domain":"notebooklm.google.com","path":"/"},{"name":"API","value":"a","domain":"notebooklm.google.com","path":"/_/LabsTailwindUi"},{"name":"OTHER","value":"x","domain":"notebooklm.google.com","path":"/other"}]}"#;
        let (_, header, _) = parse_storage(
            raw,
            "notebooklm.google.com",
            "/_/LabsTailwindUi/data/batchexecute",
        )
        .expect("credential");
        assert_eq!(header, "API=a; GLOBAL=g; ROOT=r");
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
