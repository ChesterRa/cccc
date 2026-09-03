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
    if host_override.is_none()
        && port_override.is_none()
        && let Some(binding) = live_runtime_binding(home)
    {
        return Ok(binding);
    }
    let global = settings::load(home)?;
    Ok(resolve_values(
        host_override,
        port_override,
        &global.remote_access,
        std::env::var("CCCC_WEB_HOST").ok().as_deref(),
        std::env::var("CCCC_WEB_PORT").ok().as_deref(),
    ))
}

fn live_runtime_binding(home: &HomeLayout) -> Option<WebBinding> {
    let (host, port) = cccc_daemon::live_web_binding(home)?;
    Some(WebBinding { host, port })
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn remote_access(host: &str, port: u16) -> Map<String, serde_json::Value> {
        json!({"web_host":host,"web_port":port})
            .as_object()
            .cloned()
            .expect("object")
    }

    fn verified_web_runtime(home: &HomeLayout, runtime_id: &str, proof_key: &str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Web fixture");
        let port = listener.local_addr().expect("fixture address").port();
        let response_runtime_id = runtime_id.to_owned();
        let response_proof_key = proof_key.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept readiness request");
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).expect("read readiness request");
            let request = String::from_utf8_lossy(&request[..count]);
            let target = request.split_whitespace().nth(1).expect("request target");
            let challenge = target
                .strip_prefix("/api/v1/ready?challenge=")
                .expect("challenge query");
            let proof =
                cccc_core::web_runtime_proof::sign(&response_proof_key, challenge).expect("proof");
            let body = json!({
                "ok":true,
                "result":{"web":"ready","runtime_id":response_runtime_id,"proof":proof}
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write readiness response");
        });
        cccc_core::fs::write_secret_json(
            &home.daemon_dir().join("web_runtime.json"),
            &json!({
                "pid":std::process::id(),
                "runtime_id":runtime_id,
                "runtime_proof_key":proof_key,
                "host":"127.0.0.1",
                "port":port,
            }),
        )
        .expect("runtime state");
        port
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

    #[test]
    fn live_runtime_binding_wins_without_cli_overrides() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let port = verified_web_runtime(&home, "web-live", "proof-key");

        assert_eq!(
            resolve(&home, None, None).expect("live binding"),
            WebBinding {
                host: "127.0.0.1".into(),
                port,
            }
        );
        assert_eq!(
            resolve(&home, Some("192.0.2.10"), Some(9100)).expect("explicit binding"),
            WebBinding {
                host: "192.0.2.10".into(),
                port: 9100,
            }
        );
    }

    #[test]
    fn live_binding_resolution_is_safe_inside_a_tokio_runtime_context() {
        // `im`/`space`/default launch call `resolve` inside `#[tokio::main]`;
        // the live-binding probe used to panic there with "Cannot drop a
        // runtime in a context where blocking is not allowed" (exit 101).
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let port = verified_web_runtime(&home, "web-live", "proof-key");

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let binding = runtime
            .block_on(async { resolve(&home, None, None) })
            .expect("live binding");

        assert_eq!(
            binding,
            WebBinding {
                host: "127.0.0.1".into(),
                port,
            }
        );
    }

    #[test]
    fn dead_runtime_binding_falls_back_to_saved_configuration() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.yaml"),
            "remote_access:\n  web_host: 127.0.0.2\n  web_port: 9300\n",
        )
        .expect("settings");
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserved fixture port");
        let stale_port = listener.local_addr().expect("fixture address").port();
        drop(listener);
        cccc_core::fs::write_secret_json(
            &home.daemon_dir().join("web_runtime.json"),
            &json!({
                "pid":u32::MAX,
                "runtime_id":"web-stale",
                "runtime_proof_key":"proof-key",
                "host":"127.0.0.1",
                "port":stale_port,
            }),
        )
        .expect("stale runtime state");

        assert_eq!(
            resolve(&home, None, None).expect("saved binding"),
            WebBinding {
                host: "127.0.0.2".into(),
                port: 9300,
            }
        );
    }
}
