use std::time::{SystemTime, UNIX_EPOCH};

use cookie::Cookie;
use reqwest::header::{HeaderValue, SET_COOKIE};
use serde_json::{Value, json};

use crate::error::{Error, Result};

pub(crate) fn merge_response(
    storage: &mut Value,
    response: &reqwest::blocking::Response,
) -> Result<()> {
    let host = response.url().host_str().unwrap_or(".google.com");
    merge_set_cookie_values(storage, host, response.headers().get_all(SET_COOKIE).iter())
}

fn merge_set_cookie_values<'a>(
    storage: &mut Value,
    host: &str,
    values: impl IntoIterator<Item = &'a HeaderValue>,
) -> Result<()> {
    let cookies = storage
        .get_mut("cookies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::InvalidCredential("storage state is missing cookies".into()))?;
    for header in values {
        let raw = header
            .to_str()
            .map_err(|error| Error::InvalidCredential(error.to_string()))?;
        let parsed = Cookie::parse(raw.to_owned())
            .map_err(|error| Error::InvalidCredential(error.to_string()))?;
        let domain = parsed.domain().unwrap_or(host).to_owned();
        let path = parsed.path().unwrap_or("/").to_owned();
        if parsed.max_age().is_some_and(|age| age.whole_seconds() <= 0) {
            cookies.retain(|value| !matches_cookie(value, &parsed, &domain, &path));
            continue;
        }
        if let Some(existing) = cookies
            .iter_mut()
            .find(|value| matches_cookie(value, &parsed, &domain, &path))
        {
            existing["value"] = json!(parsed.value());
            existing["secure"] = json!(parsed.secure().unwrap_or(false));
            existing["httpOnly"] = json!(parsed.http_only().unwrap_or(false));
            existing["expires"] = json!(cookie_expiry(&parsed));
        } else {
            cookies.push(json!({
                "name":parsed.name(), "value":parsed.value(), "domain":domain,
                "path":path, "secure":parsed.secure().unwrap_or(false),
                "httpOnly":parsed.http_only().unwrap_or(false),
                "expires":cookie_expiry(&parsed)
            }));
        }
    }
    Ok(())
}

fn matches_cookie(value: &Value, parsed: &Cookie<'_>, domain: &str, path: &str) -> bool {
    value["name"] == parsed.name()
        && value["domain"]
            .as_str()
            .is_some_and(|value| value.trim_start_matches('.') == domain.trim_start_matches('.'))
        && value["path"].as_str().unwrap_or("/") == path
}

fn cookie_expiry(cookie: &Cookie<'_>) -> f64 {
    if let Some(max_age) = cookie.max_age() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |duration| duration.as_secs_f64());
        return now + max_age.whole_seconds() as f64;
    }
    cookie
        .expires_datetime()
        .map_or(-1.0, |expires| expires.unix_timestamp() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Value {
        json!({"cookies":[
            {"name":"SID","value":"old","domain":".google.com","path":"/","secure":true,"httpOnly":true},
            {"name":"SID","value":"scoped","domain":".google.com","path":"/other"}
        ]})
    }

    #[test]
    fn updates_only_matching_cookie_scope() {
        let mut state = storage();
        let header =
            HeaderValue::from_static("SID=new; Domain=.google.com; Path=/; Secure; HttpOnly");
        merge_set_cookie_values(&mut state, "notebooklm.google.com", [&header])
            .expect("merge cookie");
        assert_eq!(state["cookies"][0]["value"], "new");
        assert_eq!(state["cookies"][0]["expires"], -1.0);
        assert_eq!(state["cookies"][1]["value"], "scoped");
    }

    #[test]
    fn removes_expired_cookie() {
        let mut state = storage();
        let header = HeaderValue::from_static("SID=gone; Domain=.google.com; Path=/; Max-Age=0");
        merge_set_cookie_values(&mut state, "notebooklm.google.com", [&header])
            .expect("merge cookie");
        assert_eq!(state["cookies"].as_array().expect("cookie array").len(), 1);
        assert_eq!(state["cookies"][0]["path"], "/other");
    }

    #[test]
    fn appends_host_only_cookie_without_overwriting_domain_cookie() {
        let mut state = storage();
        let header = HeaderValue::from_static("SID=host; Path=/");
        merge_set_cookie_values(&mut state, "notebooklm.google.com", [&header])
            .expect("merge cookie");
        let cookies = state["cookies"].as_array().expect("cookie array");
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[2]["domain"], "notebooklm.google.com");
    }
}
