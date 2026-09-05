//! Preserve host network routing across the managed launcher's env_clear boundary.
use serde_json::{Map, Value};
use std::{collections::BTreeMap, io};

const PROXIES: &[(&str, &str)] = &[
    ("HTTP_PROXY", "http_proxy"),
    ("HTTPS_PROXY", "https_proxy"),
    ("ALL_PROXY", "all_proxy"),
    ("NO_PROXY", "no_proxy"),
];
const CERTIFICATES: &[&str] = &["NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE", "SSL_CERT_DIR"];

pub(super) fn inherit(
    settings: &mut Map<String, Value>,
    inherited: impl IntoIterator<Item = (String, String)>,
) -> io::Result<()> {
    let inherited: BTreeMap<_, _> = inherited.into_iter().collect();
    let env = settings
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()));
    let env = env.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Claude settings env must be an object",
        )
    })?;
    for &(upper, lower) in PROXIES {
        // An explicit override in either spelling also masks inherited aliases.
        // Empty strings intentionally disable an inherited proxy.
        let value = env
            .get(upper)
            .or_else(|| env.get(lower))
            .cloned()
            .or_else(|| {
                inherited
                    .get(upper)
                    .or_else(|| inherited.get(lower))
                    .map(|value| Value::String(value.clone()))
            });
        if let Some(value) = value {
            env.insert(upper.into(), value.clone());
            env.insert(lower.into(), value);
        }
    }
    for &key in CERTIFICATES {
        if let Some(value) = inherited.get(key) {
            env.entry(key)
                .or_insert_with(|| Value::String(value.clone()));
        }
    }
    Ok(())
}

pub(super) fn extend_launcher(
    launcher: &mut BTreeMap<String, String>,
    settings: &Map<String, Value>,
) {
    let Some(env) = settings.get("env").and_then(Value::as_object) else {
        return;
    };
    for key in PROXIES
        .iter()
        .flat_map(|&(upper, lower)| [upper, lower])
        .chain(CERTIFICATES.iter().copied())
    {
        if let Some(value) = env.get(key).and_then(Value::as_str) {
            launcher.insert(key.into(), value.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_and_bootstrap_keep_routing_without_copying_unrelated_host_secrets() {
        let mut settings = json!({"env":{"CCCC_ACTOR_ID":"peer"}})
            .as_object()
            .expect("access object fixture")
            .clone();
        inherit(
            &mut settings,
            [
                ("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into()),
                ("NO_PROXY".into(), "localhost,127.0.0.1".into()),
                ("NODE_EXTRA_CA_CERTS".into(), "/private/company.pem".into()),
                ("UNRELATED_SECRET".into(), "not-for-claude".into()),
            ],
        )
        .expect("complete inherit in fixture");
        let mut launcher = BTreeMap::new();
        extend_launcher(&mut launcher, &settings);
        assert_eq!(launcher["HTTPS_PROXY"], "http://127.0.0.1:7890");
        assert_eq!(launcher["https_proxy"], launcher["HTTPS_PROXY"]);
        assert_eq!(launcher["NO_PROXY"], "localhost,127.0.0.1");
        assert_eq!(
            settings["env"]["NODE_EXTRA_CA_CERTS"],
            "/private/company.pem"
        );
        assert_eq!(settings["env"]["CCCC_ACTOR_ID"], "peer");
        assert!(settings["env"].get("UNRELATED_SECRET").is_none());
        assert!(!launcher.contains_key("CCCC_ACTOR_ID"));
    }

    #[test]
    fn explicit_lowercase_proxy_and_empty_override_mask_host_aliases() {
        for configured in ["http://127.0.0.1:8080", ""] {
            let mut settings = json!({"env":{"https_proxy":configured}})
                .as_object()
                .expect("access object fixture")
                .clone();
            inherit(
                &mut settings,
                [("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into())],
            )
            .expect("complete inherit in fixture");
            assert_eq!(settings["env"]["HTTPS_PROXY"], configured);
            assert_eq!(settings["env"]["https_proxy"], configured);
        }
    }
}
