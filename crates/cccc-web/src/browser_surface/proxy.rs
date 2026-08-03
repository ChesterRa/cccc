use anyhow::{Result, bail};

const PROXY_ENV_KEYS: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

#[derive(Debug, Eq, PartialEq)]
pub(super) struct BrowserProxy {
    server: String,
    bypass_list: Option<String>,
}

impl BrowserProxy {
    pub(super) fn from_env() -> Result<Option<Self>> {
        Self::from_values(|key| std::env::var(key).ok())
    }

    fn from_values(mut value: impl FnMut(&str) -> Option<String>) -> Result<Option<Self>> {
        let Some(raw) = PROXY_ENV_KEYS
            .iter()
            .find_map(|key| value(key).filter(|candidate| !candidate.trim().is_empty()))
        else {
            return Ok(None);
        };
        let server = normalize_proxy_server(&raw)?;
        let bypass_list = ["NO_PROXY", "no_proxy"]
            .iter()
            .find_map(|key| value(key).filter(|candidate| !candidate.trim().is_empty()))
            .and_then(|raw| normalize_bypass_list(&raw));
        Ok(Some(Self {
            server,
            bypass_list,
        }))
    }

    pub(super) fn chromium_args(&self) -> Vec<String> {
        let mut args = vec![format!("--proxy-server={}", self.server)];
        if let Some(bypass_list) = &self.bypass_list {
            args.push(format!("--proxy-bypass-list={bypass_list}"));
        }
        args
    }
}

fn normalize_proxy_server(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let candidate = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("http://{raw}")
    };
    let url = reqwest::Url::parse(&candidate)
        .map_err(|_| anyhow::anyhow!("invalid browser proxy URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("authenticated browser proxies are not supported in Chromium command-line settings");
    }
    let scheme = match url.scheme() {
        "http" | "https" | "socks4" | "socks5" => url.scheme(),
        "socks5h" => "socks5",
        _ => bail!("browser proxy must use http, https, socks4, or socks5"),
    };
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| anyhow::anyhow!("browser proxy host is required"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("browser proxy port is required"))?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    Ok(format!("{scheme}://{host}:{port}"))
}

fn normalize_bypass_list(raw: &str) -> Option<String> {
    let entries = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .strip_prefix('.')
                .map_or_else(|| entry.to_owned(), |domain| format!("*.{domain}"))
        })
        .collect::<Vec<_>>();
    (!entries.is_empty()).then(|| entries.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn from(entries: &[(&str, &str)]) -> Result<Option<BrowserProxy>> {
        let values = entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<HashMap<_, _>>();
        BrowserProxy::from_values(|key| values.get(key).cloned())
    }

    #[test]
    fn https_proxy_wins_and_no_proxy_becomes_chromium_bypass_list() {
        let proxy = from(&[
            ("ALL_PROXY", "socks5://127.0.0.1:1080"),
            ("HTTPS_PROXY", "http://127.0.0.1:7890"),
            ("NO_PROXY", "localhost, 127.0.0.1, .example.test"),
        ])
        .expect("proxy")
        .expect("configured");

        assert_eq!(
            proxy.chromium_args(),
            [
                "--proxy-server=http://127.0.0.1:7890",
                "--proxy-bypass-list=localhost;127.0.0.1;*.example.test",
            ]
        );
    }

    #[test]
    fn supports_scheme_less_and_socks5h_proxy_values() {
        assert_eq!(
            from(&[("HTTPS_PROXY", "proxy.example:8080")])
                .expect("proxy")
                .expect("configured")
                .server,
            "http://proxy.example:8080"
        );
        assert_eq!(
            from(&[("ALL_PROXY", "socks5h://127.0.0.1:1080")])
                .expect("proxy")
                .expect("configured")
                .server,
            "socks5://127.0.0.1:1080"
        );
    }

    #[test]
    fn rejects_proxy_credentials_in_process_arguments() {
        let error = from(&[("HTTPS_PROXY", "http://user:secret@proxy.example:8080")])
            .expect_err("credentials must not leak into process arguments");

        assert!(error.to_string().contains("authenticated browser proxies"));
    }
}
