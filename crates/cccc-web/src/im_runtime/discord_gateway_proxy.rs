use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use percent_encoding::percent_decode_str;
use reqwest::Url;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
#[cfg(test)]
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, accept_async_with_config, client_async_tls_with_config,
};

const DISCORD_GATEWAY_HOST: &str = "gateway.discord.gg";
const DISCORD_GATEWAY_PORT: u16 = 443;
const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROXY_HEADER_BYTES: usize = 16 * 1024;

type RemoteSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(super) struct GatewayRelay {
    pub(super) local_url: String,
    pub(super) task: JoinHandle<()>,
    latest_error: Arc<Mutex<Option<String>>>,
}

impl GatewayRelay {
    pub(super) fn latest_error(&self) -> Option<String> {
        self.latest_error
            .lock()
            .ok()
            .and_then(|error| error.clone())
    }
}

pub(super) async fn start_from_env(
    shutdown: watch::Receiver<bool>,
) -> Result<Option<GatewayRelay>, String> {
    let Some(proxy) = ProxyConfig::from_env(DISCORD_GATEWAY_HOST, DISCORD_GATEWAY_PORT)? else {
        return Ok(None);
    };
    start(proxy, DISCORD_GATEWAY_URL, shutdown).await.map(Some)
}

async fn start(
    proxy: ProxyConfig,
    remote_url: &str,
    shutdown: watch::Receiver<bool>,
) -> Result<GatewayRelay, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("failed to bind Discord gateway proxy relay: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("failed to read Discord gateway relay address: {error}"))?;
    let latest_error = Arc::new(Mutex::new(None));
    let task_error = Arc::clone(&latest_error);
    let proxy_label = proxy.label();
    let remote_url = remote_url.to_owned();
    let task = tokio::spawn(async move {
        run(listener, proxy, remote_url, task_error, shutdown).await;
    });
    tracing::info!(proxy = %proxy_label, "Discord gateway is using the configured proxy");
    Ok(GatewayRelay {
        local_url: format!("ws://{address}"),
        task,
        latest_error,
    })
}

async fn run(
    listener: TcpListener,
    proxy: ProxyConfig,
    remote_url: String,
    latest_error: Arc<Mutex<Option<String>>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let proxy = proxy.clone();
                        let remote_url = remote_url.clone();
                        let latest_error = Arc::clone(&latest_error);
                        connections.spawn(async move {
                            if let Err(error) = relay_connection(stream, &proxy, &remote_url).await {
                                if let Ok(mut latest) = latest_error.lock() {
                                    *latest = Some(error.clone());
                                }
                                tracing::warn!(%error, proxy = %proxy.label(), "Discord gateway relay failed");
                            } else if let Ok(mut latest) = latest_error.lock() {
                                *latest = None;
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Discord gateway relay listener failed");
                        break;
                    }
                }
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = result {
                    tracing::debug!(%error, "Discord gateway relay connection task stopped");
                }
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

async fn relay_connection(
    local_stream: TcpStream,
    proxy: &ProxyConfig,
    remote_url: &str,
) -> Result<(), String> {
    let mut remote = connect_remote(proxy, remote_url).await?;
    let mut local = accept_async_with_config(local_stream, Some(websocket_config()))
        .await
        .map_err(|error| format!("local Discord gateway relay handshake failed: {error}"))?;
    loop {
        tokio::select! {
            message = local.next() => {
                let Some(message) = message else { break };
                let message = message.map_err(|error| format!("local Discord gateway relay read failed: {error}"))?;
                let closing = message.is_close();
                remote.send(message).await.map_err(|error| format!("Discord gateway proxy write failed: {error}"))?;
                if closing { break; }
            }
            message = remote.next() => {
                let Some(message) = message else { break };
                let message = message.map_err(|error| format!("Discord gateway proxy read failed: {error}"))?;
                let closing = message.is_close();
                local.send(message).await.map_err(|error| format!("local Discord gateway relay write failed: {error}"))?;
                if closing { break; }
            }
        }
    }
    Ok(())
}

async fn connect_remote(proxy: &ProxyConfig, remote_url: &str) -> Result<RemoteSocket, String> {
    tokio::time::timeout(CONNECT_TIMEOUT, connect_remote_via_proxy(proxy, remote_url))
        .await
        .map_err(|_| format!("configured proxy {} timed out", proxy.label()))?
        .map_err(|error| format!("configured proxy failed: {error}"))
}

async fn connect_remote_via_proxy(
    proxy: &ProxyConfig,
    remote_url: &str,
) -> Result<RemoteSocket, String> {
    let remote =
        Url::parse(remote_url).map_err(|error| format!("invalid Discord gateway URL: {error}"))?;
    let target_host = remote
        .host_str()
        .ok_or_else(|| "Discord gateway host is missing".to_owned())?;
    let target_port = remote
        .port_or_known_default()
        .ok_or_else(|| "Discord gateway port is missing".to_owned())?;
    let tunnel = proxy.open_tunnel(target_host, target_port).await?;
    client_async_tls_with_config(remote_url, tunnel, Some(websocket_config()), None)
        .await
        .map(|(socket, _)| socket)
        .map_err(|error| {
            format!("Discord gateway WebSocket handshake through proxy failed: {error}")
        })
}

fn websocket_config() -> WebSocketConfig {
    let mut config = WebSocketConfig::default();
    config.max_message_size = None;
    config.max_frame_size = None;
    config
}

#[derive(Clone, Copy)]
enum ProxyKind {
    Http,
    Socks5,
}

#[derive(Clone)]
struct ProxyConfig {
    kind: ProxyKind,
    host: String,
    port: u16,
    username: String,
    password: String,
}

impl ProxyConfig {
    fn from_env(target_host: &str, target_port: u16) -> Result<Option<Self>, String> {
        Self::from_values(|key| std::env::var(key).ok(), target_host, target_port)
    }

    fn from_values(
        value: impl Fn(&str) -> Option<String>,
        target_host: &str,
        target_port: u16,
    ) -> Result<Option<Self>, String> {
        let no_proxy = value("no_proxy").or_else(|| value("NO_PROXY"));
        if no_proxy
            .as_deref()
            .is_some_and(|rules| bypasses_proxy(rules, target_host, target_port))
        {
            return Ok(None);
        }
        let selected = [
            "https_proxy",
            "HTTPS_PROXY",
            "http_proxy",
            "HTTP_PROXY",
            "all_proxy",
            "ALL_PROXY",
        ]
        .into_iter()
        .find_map(|key| {
            value(key)
                .filter(|candidate| !candidate.trim().is_empty())
                .map(|candidate| (key, candidate))
        });
        let Some((source, raw)) = selected else {
            return Ok(None);
        };
        let normalized = if raw.contains("://") {
            raw.trim().to_owned()
        } else {
            format!("http://{}", raw.trim())
        };
        let url = Url::parse(&normalized)
            .map_err(|_| format!("invalid Discord gateway proxy URL in {source}"))?;
        let kind = match url.scheme() {
            "http" => ProxyKind::Http,
            "socks5" | "socks5h" => ProxyKind::Socks5,
            scheme => {
                return Err(format!(
                    "unsupported Discord gateway proxy scheme {scheme}; use http:// or socks5://"
                ));
            }
        };
        let host = url
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or_else(|| format!("Discord gateway proxy host is missing in {source}"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("Discord gateway proxy port is missing in {source}"))?;
        Ok(Some(Self {
            kind,
            host: host.to_owned(),
            port,
            username: decode_userinfo(url.username()),
            password: decode_userinfo(url.password().unwrap_or_default()),
        }))
    }

    fn label(&self) -> String {
        let scheme = match self.kind {
            ProxyKind::Http => "http",
            ProxyKind::Socks5 => "socks5",
        };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    async fn open_tunnel(&self, target_host: &str, target_port: u16) -> Result<TcpStream, String> {
        let stream = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|error| format!("could not connect to proxy {}: {error}", self.label()))?;
        match self.kind {
            ProxyKind::Http => {
                self.open_http_tunnel(stream, target_host, target_port)
                    .await
            }
            ProxyKind::Socks5 => {
                self.open_socks5_tunnel(stream, target_host, target_port)
                    .await
            }
        }
    }

    async fn open_http_tunnel(
        &self,
        mut stream: TcpStream,
        target_host: &str,
        target_port: u16,
    ) -> Result<TcpStream, String> {
        let authority = format!("{target_host}:{target_port}");
        let authorization = if self.username.is_empty() {
            String::new()
        } else {
            let credentials = STANDARD.encode(format!("{}:{}", self.username, self.password));
            format!("Proxy-Authorization: Basic {credentials}\r\n")
        };
        let request = format!(
            "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n{authorization}\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| format!("failed to write HTTP CONNECT request: {error}"))?;
        let header = read_http_header(&mut stream).await?;
        let status_line = header
            .lines()
            .next()
            .ok_or_else(|| "HTTP proxy returned an empty response".to_owned())?;
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| "HTTP proxy returned an invalid status line".to_owned())?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "HTTP proxy CONNECT was rejected with status {status}"
            ));
        }
        Ok(stream)
    }

    async fn open_socks5_tunnel(
        &self,
        mut stream: TcpStream,
        target_host: &str,
        target_port: u16,
    ) -> Result<TcpStream, String> {
        let has_credentials = !self.username.is_empty();
        let methods: &[u8] = if has_credentials {
            &[0x00, 0x02]
        } else {
            &[0x00]
        };
        let mut greeting = vec![0x05, methods.len() as u8];
        greeting.extend_from_slice(methods);
        stream
            .write_all(&greeting)
            .await
            .map_err(|error| format!("failed to write SOCKS5 greeting: {error}"))?;
        let mut choice = [0_u8; 2];
        stream
            .read_exact(&mut choice)
            .await
            .map_err(|error| format!("failed to read SOCKS5 greeting: {error}"))?;
        if choice[0] != 0x05 {
            return Err("SOCKS5 proxy returned an invalid protocol version".into());
        }
        match choice[1] {
            0x00 => {}
            0x02 if has_credentials => self.authenticate_socks5(&mut stream).await?,
            0xff => return Err("SOCKS5 proxy has no acceptable authentication method".into()),
            method => {
                return Err(format!(
                    "SOCKS5 proxy selected unsupported authentication method {method:#04x}"
                ));
            }
        }
        let host = target_host.as_bytes();
        if host.len() > u8::MAX as usize {
            return Err("Discord gateway hostname is too long for SOCKS5".into());
        }
        let mut request = vec![0x05, 0x01, 0x00, 0x03, host.len() as u8];
        request.extend_from_slice(host);
        request.extend_from_slice(&target_port.to_be_bytes());
        stream
            .write_all(&request)
            .await
            .map_err(|error| format!("failed to write SOCKS5 CONNECT request: {error}"))?;
        read_socks5_connect_response(&mut stream).await?;
        Ok(stream)
    }

    async fn authenticate_socks5(&self, stream: &mut TcpStream) -> Result<(), String> {
        let username = self.username.as_bytes();
        let password = self.password.as_bytes();
        if username.len() > u8::MAX as usize || password.len() > u8::MAX as usize {
            return Err("SOCKS5 proxy credentials are too long".into());
        }
        let mut request = vec![0x01, username.len() as u8];
        request.extend_from_slice(username);
        request.push(password.len() as u8);
        request.extend_from_slice(password);
        stream
            .write_all(&request)
            .await
            .map_err(|error| format!("failed to write SOCKS5 authentication: {error}"))?;
        let mut response = [0_u8; 2];
        stream
            .read_exact(&mut response)
            .await
            .map_err(|error| format!("failed to read SOCKS5 authentication: {error}"))?;
        if response != [0x01, 0x00] {
            return Err("SOCKS5 proxy authentication failed".into());
        }
        Ok(())
    }
}

async fn read_http_header(stream: &mut TcpStream) -> Result<String, String> {
    let mut header = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !header.windows(4).any(|window| window == b"\r\n\r\n") {
        if header.len() >= MAX_PROXY_HEADER_BYTES {
            return Err("HTTP proxy response headers are too large".into());
        }
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| format!("failed to read HTTP proxy response: {error}"))?;
        if read == 0 {
            return Err("HTTP proxy closed before completing CONNECT".into());
        }
        header.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(header).map_err(|_| "HTTP proxy returned non-UTF-8 headers".into())
}

async fn read_socks5_connect_response(stream: &mut TcpStream) -> Result<(), String> {
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|error| format!("failed to read SOCKS5 CONNECT response: {error}"))?;
    if header[0] != 0x05 {
        return Err("SOCKS5 proxy returned an invalid protocol version".into());
    }
    if header[1] != 0x00 {
        return Err(format!(
            "SOCKS5 proxy CONNECT failed with status {:#04x}",
            header[1]
        ));
    }
    let address_bytes = match header[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut length = [0_u8; 1];
            stream
                .read_exact(&mut length)
                .await
                .map_err(|error| format!("failed to read SOCKS5 address length: {error}"))?;
            usize::from(length[0])
        }
        kind => {
            return Err(format!(
                "SOCKS5 proxy returned invalid address type {kind:#04x}"
            ));
        }
    };
    let mut remainder = vec![0_u8; address_bytes + 2];
    stream
        .read_exact(&mut remainder)
        .await
        .map_err(|error| format!("failed to read SOCKS5 bound address: {error}"))?;
    Ok(())
}

fn decode_userinfo(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

fn bypasses_proxy(rules: &str, target_host: &str, target_port: u16) -> bool {
    let target_host = target_host.trim_matches(['[', ']']).to_ascii_lowercase();
    rules.split(',').any(|rule| {
        let mut rule = rule.trim().to_ascii_lowercase();
        if rule.is_empty() {
            return false;
        }
        if rule == "*" {
            return true;
        }
        if let Some((host, port)) = rule.rsplit_once(':')
            && port.parse::<u16>().is_ok()
        {
            if port.parse::<u16>().ok() != Some(target_port) {
                return false;
            }
            rule = host.to_owned();
        }
        let rule = rule.trim_start_matches('.');
        target_host == rule || target_host.ends_with(&format!(".{rule}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::copy_bidirectional;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn resolves_https_proxy_without_exposing_credentials() {
        let proxy = ProxyConfig::from_values(
            |key| (key == "HTTPS_PROXY").then(|| "http://user:secret@127.0.0.1:7890".to_owned()),
            DISCORD_GATEWAY_HOST,
            DISCORD_GATEWAY_PORT,
        )
        .expect("proxy config")
        .expect("proxy");

        assert_eq!(proxy.label(), "http://127.0.0.1:7890");
        assert!(!proxy.label().contains("user"));
        assert!(!proxy.label().contains("secret"));
    }

    #[test]
    fn no_proxy_bypasses_exact_and_parent_domains() {
        for rules in ["gateway.discord.gg", ".discord.gg", "discord.gg"] {
            let proxy = ProxyConfig::from_values(
                |key| match key {
                    "HTTPS_PROXY" => Some("http://127.0.0.1:7890".to_owned()),
                    "NO_PROXY" => Some(rules.to_owned()),
                    _ => None,
                },
                DISCORD_GATEWAY_HOST,
                DISCORD_GATEWAY_PORT,
            )
            .expect("proxy config");
            assert!(proxy.is_none(), "rules={rules}");
        }
    }

    #[test]
    fn http_proxy_precedes_the_generic_all_proxy_fallback() {
        let proxy = ProxyConfig::from_values(
            |key| match key {
                "HTTP_PROXY" => Some("http://127.0.0.1:7890".to_owned()),
                "ALL_PROXY" => Some("socks5://127.0.0.1:1080".to_owned()),
                _ => None,
            },
            DISCORD_GATEWAY_HOST,
            DISCORD_GATEWAY_PORT,
        )
        .expect("proxy config")
        .expect("proxy");

        assert_eq!(proxy.label(), "http://127.0.0.1:7890");
    }

    #[tokio::test]
    async fn configured_proxy_failure_never_falls_back_to_direct_connection() {
        let remote_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("remote listener");
        let remote_address = remote_listener.local_addr().expect("remote address");
        let (direct_contact_tx, direct_contact_rx) = tokio::sync::oneshot::channel();
        let remote = tokio::spawn(async move {
            let (stream, _) = remote_listener.accept().await.expect("remote accept");
            let _ = direct_contact_tx.send(());
            let _ = accept_async(stream).await;
        });

        let proxy_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("proxy listener");
        let proxy_address = proxy_listener.local_addr().expect("proxy address");
        let proxy_task = tokio::spawn(async move {
            let (mut inbound, _) = proxy_listener.accept().await.expect("proxy accept");
            let _ = read_http_header(&mut inbound)
                .await
                .expect("CONNECT request");
            inbound
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .await
                .expect("CONNECT rejection");
        });
        let proxy = ProxyConfig {
            kind: ProxyKind::Http,
            host: proxy_address.ip().to_string(),
            port: proxy_address.port(),
            username: String::new(),
            password: String::new(),
        };

        let result = connect_remote(&proxy, &format!("ws://{remote_address}")).await;

        assert!(result.is_err(), "configured proxy must be fail-closed");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), direct_contact_rx)
                .await
                .is_err(),
            "the direct endpoint must not be contacted"
        );
        remote.abort();
        proxy_task.await.expect("proxy task");
    }

    #[tokio::test]
    async fn relays_websocket_frames_through_http_connect_proxy() {
        let remote_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("remote listener");
        let remote_address = remote_listener.local_addr().expect("remote address");
        let remote = tokio::spawn(async move {
            let (stream, _) = remote_listener.accept().await.expect("remote accept");
            let mut socket = accept_async(stream).await.expect("remote websocket");
            let message = socket
                .next()
                .await
                .expect("remote message")
                .expect("remote frame");
            assert_eq!(message.into_text().expect("text"), "ping");
            socket
                .send(Message::Text("pong".into()))
                .await
                .expect("remote send");
            while let Some(message) = socket.next().await {
                match message {
                    Ok(message) if message.is_close() => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        let proxy_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("proxy listener");
        let proxy_address = proxy_listener.local_addr().expect("proxy address");
        let proxy_task = tokio::spawn(async move {
            let (mut inbound, _) = proxy_listener.accept().await.expect("proxy accept");
            let request = read_http_header(&mut inbound)
                .await
                .expect("CONNECT request");
            assert!(
                request.starts_with(&format!("CONNECT {remote_address} HTTP/1.1")),
                "{request}"
            );
            let mut outbound = TcpStream::connect(remote_address)
                .await
                .expect("remote connect");
            inbound
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .expect("CONNECT response");
            copy_bidirectional(&mut inbound, &mut outbound)
                .await
                .expect("proxy relay");
        });

        let proxy = ProxyConfig {
            kind: ProxyKind::Http,
            host: proxy_address.ip().to_string(),
            port: proxy_address.port(),
            username: String::new(),
            password: String::new(),
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let relay = start(proxy, &format!("ws://{remote_address}"), shutdown_rx)
            .await
            .expect("gateway relay");
        let (mut client, _) =
            connect_async_with_config(&relay.local_url, Some(websocket_config()), false)
                .await
                .expect("local client");
        client
            .send(Message::Text("ping".into()))
            .await
            .expect("local send");
        let response = client
            .next()
            .await
            .expect("local response")
            .expect("local frame");
        assert_eq!(response.into_text().expect("response text"), "pong");
        assert!(relay.latest_error().is_none());

        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(2), relay.task)
            .await
            .expect("relay shutdown")
            .expect("relay task");
        remote.await.expect("remote task");
        proxy_task.abort();
        let _ = proxy_task.await;
    }
}
