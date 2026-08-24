use reqwest::Method;
use reqwest::blocking::Client;
use serde_json::{Map, Value, json};
use std::io::Read;
use std::time::Duration;
use url::{Host, Url};

const CLIENT_VERSION: &str = "1";
const VERSION_HEADER: &str = "CCCC-Membership-Version";
const USER_AGENT: &str = "cccc-membership";
const DEFAULT_TIMEOUT_SECONDS: f64 = 15.0;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(super) struct AccountError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub retry_after_delta: u64,
    pub terminal_authorization: bool,
}

impl AccountError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            retry_after_delta: 0,
            terminal_authorization: false,
        }
    }

    fn terminal(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            retry_after_delta: 0,
            terminal_authorization: true,
        }
    }

    fn retry(message: impl Into<String>, retry_after_delta: u64) -> Self {
        Self {
            code: "membership_authorization_pending",
            message: message.into(),
            retryable: true,
            retry_after_delta,
            terminal_authorization: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct DeviceLogin {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone)]
pub(super) struct DeviceGrant {
    pub device_token: String,
    pub device_id: String,
    pub hostname: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ReachCredentials {
    pub hostname: String,
    pub tunnel_token: String,
}

#[derive(Debug, Clone)]
pub(super) struct DeviceStatus {
    pub device_id: Option<String>,
    pub hostname: Option<String>,
    pub disabled: bool,
    pub online: Option<bool>,
}

pub(super) struct AccountClient {
    origin: Url,
    client: Client,
}

pub(super) fn canonical_reach_hostname(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || value.contains(['%', '\\'])
    {
        return None;
    }
    let candidate = if value.contains("://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let parsed = Url::parse(&candidate).ok()?;
    let host = parsed.host()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    if let Host::Domain(hostname) = host {
        let dns_hostname = hostname.strip_suffix('.').unwrap_or(hostname);
        if dns_hostname.is_empty()
            || dns_hostname.len() > 253
            || dns_hostname.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|character| character.is_ascii_alphanumeric() || character == b'-')
            })
        {
            return None;
        }
    }
    Some(parsed.origin().ascii_serialization())
}

impl AccountClient {
    pub fn new(origin: &str) -> Result<Self, AccountError> {
        Self::with_timeout(origin, None)
    }

    pub fn with_timeout(origin: &str, timeout_seconds: Option<f64>) -> Result<Self, AccountError> {
        let origin = origin.trim().trim_end_matches('/');
        let mut parsed = Url::parse(origin).map_err(|_| {
            AccountError::new(
                "membership_unavailable",
                "CCCC_ACCOUNT_ORIGIN must be an absolute http(s) origin",
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || !matches!(parsed.path(), "" | "/")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(AccountError::new(
                "membership_unavailable",
                "CCCC_ACCOUNT_ORIGIN must be an absolute http(s) origin",
            ));
        }
        parsed.set_path("/");
        let timeout = timeout_seconds
            .filter(|value| value.is_finite())
            .or_else(|| {
                std::env::var("CCCC_ACCOUNT_TIMEOUT_S")
                    .ok()
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite())
            })
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .max(0.2);
        let loopback = parsed.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
        if parsed.scheme() == "http" && !loopback {
            return Err(AccountError::new(
                "membership_unavailable",
                "CCCC_ACCOUNT_ORIGIN must use HTTPS except for a loopback development server",
            ));
        }
        let mut builder = Client::builder()
            .timeout(Duration::from_secs_f64(timeout))
            .redirect(reqwest::redirect::Policy::none());
        if loopback {
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(network_error)?;
        Ok(Self {
            origin: parsed,
            client,
        })
    }

    pub fn start_device_login(&self) -> Result<DeviceLogin, AccountError> {
        let data = self.request(Method::POST, "/v1/device/code", Some(json!({})), None)?;
        let device_code = text(&data, "device_code");
        let user_code = text(&data, "user_code");
        let verification_uri = non_blank(&data, "verification_uri")
            .or_else(|| non_blank(&data, "verification_uri_complete"))
            .unwrap_or_default();
        let verification_uri_complete = non_blank(&data, "verification_uri_complete");
        if device_code.is_empty() || user_code.is_empty() || verification_uri.is_empty() {
            return Err(AccountError::new(
                "membership_network",
                "account service returned an incomplete device code",
            ));
        }
        let verification_uri = self.authorization_url(&verification_uri)?;
        let verification_uri_complete = verification_uri_complete
            .as_deref()
            .map(|value| self.authorization_url(value))
            .transpose()?;
        Ok(DeviceLogin {
            device_code,
            user_code,
            verification_uri,
            verification_uri_complete,
            expires_in: integer(&data, "expires_in", 900).max(30),
            interval: integer(&data, "interval", 5).max(1),
        })
    }

    fn authorization_url(&self, value: &str) -> Result<String, AccountError> {
        let parsed = Url::parse(value).map_err(|_| {
            AccountError::new(
                "membership_network",
                "account service returned an invalid device authorization URL",
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || parsed.scheme() != self.origin.scheme()
            || parsed.host_str() != self.origin.host_str()
            || parsed.port_or_known_default() != self.origin.port_or_known_default()
        {
            return Err(AccountError::new(
                "membership_network",
                "account service returned an off-origin device authorization URL",
            ));
        }
        Ok(parsed.to_string())
    }

    pub fn poll_device_login(&self, device_code: &str) -> Result<DeviceGrant, AccountError> {
        let data = self.request(
            Method::POST,
            "/v1/device/token",
            Some(json!({
                "grant_type":"urn:ietf:params:oauth:grant-type:device_code",
                "device_code":device_code,
            })),
            None,
        )?;
        let device_token = non_blank(&data, "access_token")
            .or_else(|| non_blank(&data, "device_token"))
            .unwrap_or_default();
        let device_id = text(&data, "device_id");
        if device_token.is_empty() || device_id.is_empty() {
            return Err(AccountError::new(
                "membership_network",
                "account service returned an incomplete device grant",
            ));
        }
        let raw_hostname = non_blank(&data, "hostname");
        let hostname = raw_hostname.as_deref().and_then(canonical_reach_hostname);
        if raw_hostname.is_some() && hostname.is_none() {
            return Err(AccountError::new(
                "membership_network",
                "account service returned an unsafe reach hostname",
            ));
        }
        Ok(DeviceGrant {
            device_token,
            device_id,
            hostname,
        })
    }

    pub fn issue_reach(
        &self,
        device_token: &str,
        origin_port: u16,
    ) -> Result<ReachCredentials, AccountError> {
        let data = self.request(
            Method::POST,
            "/v1/reach",
            Some(json!({"origin_port":origin_port})),
            Some(device_token),
        )?;
        let hostname = non_blank(&data, "hostname")
            .as_deref()
            .and_then(canonical_reach_hostname);
        let tunnel_token = text(&data, "tunnel_token");
        if hostname.is_none() || tunnel_token.is_empty() {
            return Err(AccountError::new(
                "membership_network",
                "account service returned incomplete or unsafe reach credentials",
            ));
        }
        Ok(ReachCredentials {
            hostname: hostname.expect("hostname was validated"),
            tunnel_token,
        })
    }

    pub fn fetch_device(&self, device_token: &str) -> Result<DeviceStatus, AccountError> {
        let data = self.request(Method::GET, "/v1/device", None, Some(device_token))?;
        Ok(DeviceStatus {
            device_id: non_blank(&data, "device_id"),
            hostname: non_blank(&data, "hostname")
                .as_deref()
                .and_then(canonical_reach_hostname),
            disabled: data
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            online: data.get("online").and_then(Value::as_bool),
        })
    }

    pub fn disable_device(&self, device_token: &str) -> Result<(), AccountError> {
        self.request(
            Method::POST,
            "/v1/device/disable",
            Some(json!({})),
            Some(device_token),
        )?;
        Ok(())
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
        token: Option<&str>,
    ) -> Result<Map<String, Value>, AccountError> {
        let url = self
            .origin
            .join(path.trim_start_matches('/'))
            .map_err(network_error)?;
        let mut request = self
            .client
            .request(method, url)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .header(VERSION_HEADER, CLIENT_VERSION);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let mut response = request.send().map_err(network_error)?;
        let status = response.status().as_u16();
        let mut raw = Vec::new();
        response
            .by_ref()
            .take((MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut raw)
            .map_err(network_error)?;
        if raw.len() > MAX_RESPONSE_BYTES {
            return Err(AccountError::new(
                "membership_network",
                "account service response exceeded size limit",
            ));
        }
        let data = if raw.is_empty() {
            Map::new()
        } else {
            serde_json::from_slice::<Value>(&raw)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .ok_or_else(|| {
                    AccountError::new(
                        "membership_network",
                        "account service returned a non-JSON body",
                    )
                })?
        };
        if !(200..300).contains(&status) {
            return Err(error_from_payload(status, &data));
        }
        Ok(data)
    }
}

fn error_from_payload(status: u16, payload: &Map<String, Value>) -> AccountError {
    let nested = payload.get("error");
    let (code, message) = match nested {
        Some(Value::Object(error)) => (
            text(error, "code"),
            non_blank(error, "message").unwrap_or_default(),
        ),
        Some(Value::String(error)) => (
            error.trim().to_owned(),
            non_blank(payload, "error_description").unwrap_or_else(|| error.trim().to_owned()),
        ),
        _ => (String::new(), String::new()),
    };
    match code.as_str() {
        "authorization_pending" => {
            AccountError::retry(nonempty_message(message, "authorization_pending"), 0)
        }
        "slow_down" => AccountError::retry(nonempty_message(message, "slow_down"), 5),
        "expired_token" | "expired" => AccountError::terminal(
            "membership_network",
            nonempty_message(message, "device code expired"),
        ),
        "access_denied" | "denied" => AccountError::terminal(
            "membership_gate",
            nonempty_message(message, "login was denied"),
        ),
        "unsupported_version" | "version_unsupported" => AccountError::new(
            "membership_unsupported_version",
            nonempty_message(message, "please upgrade CCCC"),
        ),
        "disabled" | "device_disabled" => AccountError::new(
            "membership_disabled",
            nonempty_message(message, "this device has been disabled"),
        ),
        _ if status == 426 => AccountError::new(
            "membership_unsupported_version",
            nonempty_message(message, "please upgrade CCCC"),
        ),
        _ if status == 403 => AccountError::new(
            "membership_disabled",
            nonempty_message(message, "this device has been disabled"),
        ),
        _ if matches!(status, 401 | 404) => AccountError::new(
            "membership_not_logged_in",
            nonempty_message(message, "not logged in"),
        ),
        _ => AccountError::new(
            "membership_network",
            nonempty_message(
                message,
                &format!("account service rejected the request ({status})"),
            ),
        ),
    }
}

fn network_error(error: impl std::fmt::Display) -> AccountError {
    AccountError::new(
        "membership_network",
        format!("account service is not reachable: {error}"),
    )
}

fn nonempty_message(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.into()
    } else {
        value
    }
}

fn non_blank(values: &Map<String, Value>, key: &str) -> Option<String> {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn text(values: &Map<String, Value>, key: &str) -> String {
    non_blank(values, key).unwrap_or_default()
}

fn integer(values: &Map<String, Value>, key: &str, default: u64) -> u64 {
    values
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    fn server(responses: Vec<(u16, &'static str)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let origin = format!("http://{address}");
        let response_origin = origin.clone();
        thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request);
                let reason = if status == 200 { "OK" } else { "Bad Request" };
                let body = body.replace("$ORIGIN", &response_origin);
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("response");
            }
        });
        origin
    }

    #[test]
    fn login_pending_and_grant_follow_the_python_contract() {
        let origin = server(vec![
            (
                200,
                r#"{"device_code":"dc-1","user_code":"ABCD-EFGH","verification_uri":"$ORIGIN/device","verification_uri_complete":"$ORIGIN/device?user_code=ABCD-EFGH","expires_in":600,"interval":120}"#,
            ),
            (400, r#"{"error":"authorization_pending"}"#),
            (
                200,
                r#"{"access_token":"token","device_id":"d-1","hostname":"https://d-1.example.test"}"#,
            ),
        ]);
        let client = AccountClient::new(&origin).expect("client");
        let login = client.start_device_login().expect("login");
        assert_eq!(login.interval, 120);
        let expected_approval = format!("{origin}/device?user_code=ABCD-EFGH");
        assert_eq!(
            login.verification_uri_complete.as_deref(),
            Some(expected_approval.as_str())
        );
        assert!(
            client
                .poll_device_login(&login.device_code)
                .expect_err("pending")
                .retryable
        );
        let grant = client.poll_device_login(&login.device_code).expect("grant");
        assert_eq!(grant.device_token, "token");
        assert_eq!(grant.device_id, "d-1");
    }

    #[test]
    fn slow_down_adds_five_seconds() {
        let origin = server(vec![(400, r#"{"error":{"code":"slow_down"}}"#)]);
        let error = AccountClient::new(&origin)
            .expect("client")
            .poll_device_login("dc-1")
            .expect_err("slow down");
        assert!(error.retryable);
        assert_eq!(error.retry_after_delta, 5);
    }

    #[test]
    fn redirects_are_rejected_as_account_errors() {
        let origin = server(vec![(302, r#"{}"#)]);
        let error = AccountClient::new(&origin)
            .expect("client")
            .start_device_login()
            .expect_err("redirect");
        assert_eq!(error.code, "membership_network");
    }

    #[test]
    fn off_origin_device_authorization_urls_are_rejected() {
        let origin = server(vec![(
            200,
            r#"{"device_code":"dc-1","user_code":"ABCD-EFGH","verification_uri":"https://attacker.example.test/device","expires_in":600,"interval":5}"#,
        )]);
        let error = AccountClient::new(&origin)
            .expect("client")
            .start_device_login()
            .expect_err("off-origin approval URL");
        assert_eq!(error.code, "membership_network");
        assert!(error.message.contains("off-origin"));
    }

    #[test]
    fn plain_http_is_limited_to_loopback_development_servers() {
        let error = AccountClient::new("http://account.example.test")
            .err()
            .expect("plain HTTP");
        assert_eq!(error.code, "membership_unavailable");
        assert!(AccountClient::new("http://127.0.0.1:8787").is_ok());
    }

    #[test]
    fn account_origin_rejects_paths_queries_fragments_and_userinfo() {
        for origin in [
            "https://account.example.test/device",
            "https://account.example.test/?tenant=one",
            "https://user@account.example.test",
            "https://account.example.test/#fragment",
        ] {
            let error = AccountClient::new(origin).err().expect("non-origin URL");
            assert_eq!(error.code, "membership_unavailable", "{origin}");
        }
    }

    #[test]
    fn reach_and_device_status_are_typed() {
        let origin = server(vec![
            (
                200,
                r#"{"hostname":"https://d-1.example.test","tunnel_token":"tunnel"}"#,
            ),
            (
                200,
                r#"{"device_id":"d-1","hostname":"https://d-1.example.test","disabled":false,"online":true}"#,
            ),
        ]);
        let client = AccountClient::new(&origin).expect("client");
        assert_eq!(
            client
                .issue_reach("token", 9000)
                .expect("reach")
                .tunnel_token,
            "tunnel"
        );
        let device = client.fetch_device("token").expect("device");
        assert_eq!(device.device_id.as_deref(), Some("d-1"));
        assert!(!device.disabled);
    }

    #[test]
    fn device_can_retire_its_remote_credential() {
        let origin = server(vec![(200, r#"{"device_id":"d-1","disabled":true}"#)]);
        AccountClient::new(&origin)
            .expect("client")
            .disable_device("token")
            .expect("disable device");
    }

    #[test]
    fn reach_rejects_non_https_or_non_origin_hostnames() {
        for hostname in [
            "http://attacker.example.test",
            "https://user@attacker.example.test",
            "https://attacker.example.test/path",
            "https://attacker.example.test/?redirect=1",
            "https://attacker.example.test:not-a-port",
        ] {
            let body = format!(r#"{{"hostname":"{hostname}","tunnel_token":"tunnel"}}"#);
            let leaked: &'static str = Box::leak(body.into_boxed_str());
            let origin = server(vec![(200, leaked)]);
            let error = AccountClient::new(&origin)
                .expect("client")
                .issue_reach("token", 9000)
                .expect_err("unsafe hostname");
            assert_eq!(error.code, "membership_network", "{hostname}");
        }
    }

    #[test]
    fn reach_hostname_normalization_is_canonical_and_strict() {
        assert_eq!(
            canonical_reach_hostname("HTTPS://D-AbC.Example.Test:443/"),
            Some("https://d-abc.example.test".to_owned())
        );
        assert_eq!(
            canonical_reach_hostname("https://[2001:db8::1]:8443"),
            Some("https://[2001:db8::1]:8443".to_owned())
        );
        for hostname in [
            "https://exa mple.example.test",
            "https://%65xample.example.test",
            "https://_service.example.test",
            "https://-host.example.test",
            "https://host-.example.test",
            "https://example.test\\evil",
        ] {
            assert_eq!(canonical_reach_hostname(hostname), None, "{hostname}");
        }
    }
}
