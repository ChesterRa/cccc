use anyhow::Result;
use cccc_core::{HomeLayout, settings};
use serde_json::Map;

const DEFAULT_WEB_HOST: &str = "127.0.0.1";
const DEFAULT_WEB_PORT: u16 = 8848;

#[derive(Debug, PartialEq, Eq)]
pub struct WebBinding {
    pub host: String,
    pub port: u16,
}

pub fn resolve(
    home: &HomeLayout,
    host_override: Option<&str>,
    port_override: Option<u16>,
) -> Result<WebBinding> {
    let global = settings::load(home)?;
    Ok(resolve_values(
        host_override,
        port_override,
        &global.remote_access,
        std::env::var("CCCC_WEB_HOST").ok().as_deref(),
        std::env::var("CCCC_WEB_PORT").ok().as_deref(),
    ))
}

fn resolve_values(
    host_override: Option<&str>,
    port_override: Option<u16>,
    remote_access: &Map<String, serde_json::Value>,
    env_host: Option<&str>,
    env_port: Option<&str>,
) -> WebBinding {
    let saved_host = remote_access
        .get("web_host")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let host = nonempty(host_override)
        .or(saved_host)
        .or_else(|| nonempty(env_host))
        .unwrap_or(DEFAULT_WEB_HOST)
        .to_owned();

    let saved_port = remote_access
        .get("web_port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0);
    let env_port = env_port
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|value| *value > 0);
    let port = port_override
        .or(saved_port)
        .or(env_port)
        .unwrap_or(DEFAULT_WEB_PORT);

    WebBinding { host, port }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn remote_access(host: &str, port: u16) -> Map<String, serde_json::Value> {
        json!({"web_host":host,"web_port":port})
            .as_object()
            .cloned()
            .expect("object")
    }

    #[test]
    fn resolves_saved_binding_before_environment() {
        assert_eq!(
            resolve_values(
                None,
                None,
                &remote_access("0.0.0.0", 9000),
                Some("127.0.0.2"),
                Some("9001"),
            ),
            WebBinding {
                host: "0.0.0.0".into(),
                port: 9000,
            }
        );
    }

    #[test]
    fn explicit_overrides_win_over_saved_binding() {
        assert_eq!(
            resolve_values(
                Some("192.0.2.10"),
                Some(9100),
                &remote_access("0.0.0.0", 9000),
                Some("127.0.0.2"),
                Some("9001"),
            ),
            WebBinding {
                host: "192.0.2.10".into(),
                port: 9100,
            }
        );
    }

    #[test]
    fn falls_back_to_environment_then_defaults() {
        let empty = Map::new();
        assert_eq!(
            resolve_values(None, None, &empty, Some("0.0.0.0"), Some("9200")),
            WebBinding {
                host: "0.0.0.0".into(),
                port: 9200,
            }
        );
        assert_eq!(
            resolve_values(None, None, &empty, None, None),
            WebBinding {
                host: DEFAULT_WEB_HOST.into(),
                port: DEFAULT_WEB_PORT,
            }
        );
    }

    #[test]
    fn resolves_binding_from_legacy_python_settings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  web_host: 0.0.0.0\n  web_port: 9300\n",
        )
        .expect("legacy settings");

        assert_eq!(
            resolve(&home, None, None).expect("binding"),
            WebBinding {
                host: "0.0.0.0".into(),
                port: 9300,
            }
        );
    }
}
