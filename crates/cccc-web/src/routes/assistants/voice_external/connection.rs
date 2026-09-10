use super::{AsrError, Provider, bailian, config::Config, transcript::Event, volcengine};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Message, client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig,
    },
};

pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub(super) type Writer = futures_util::stream::SplitSink<Socket, Message>;
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const FINISH_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) struct Codec {
    pub provider: Provider,
    pub task: String,
}
pub(super) struct Opened {
    pub socket: Socket,
    pub codec: Codec,
    pub model: String,
}

impl Codec {
    pub fn audio(&self, bytes: &[u8]) -> Result<Message, AsrError> {
        match self.provider {
            Provider::Bailian => Ok(Message::Binary(bytes.to_vec().into())),
            Provider::Volcengine => volcengine::audio(bytes, false),
        }
    }
    pub fn finish(&self) -> Result<Message, AsrError> {
        match self.provider {
            Provider::Bailian => Ok(bailian::finish(&self.task)),
            Provider::Volcengine => volcengine::audio(&[], true),
        }
    }
    pub fn parse(&self, message: Message) -> Result<(Event, bool), AsrError> {
        match message {
            Message::Text(text) if self.provider == Provider::Bailian => {
                self.bailian(text.as_bytes())
            }
            Message::Binary(bytes) if self.provider == Provider::Bailian => self.bailian(&bytes),
            Message::Binary(bytes) => volcengine::parse(&bytes),
            Message::Ping(_) | Message::Pong(_) => Ok((Event::Ignore, false)),
            Message::Close(_) => Err(AsrError::new(
                "external_asr_disconnected",
                "The ASR provider disconnected before recognition completed",
            )),
            _ => Err(AsrError::new(
                "external_asr_protocol_error",
                "Unexpected ASR provider message",
            )),
        }
    }
    fn bailian(&self, bytes: &[u8]) -> Result<(Event, bool), AsrError> {
        let event = bailian::parse(bytes, &self.task)?;
        let done = matches!(event, Event::Finished);
        Ok((event, done))
    }
}

pub(super) async fn connect(
    provider: Provider,
    config: Config,
    language: &str,
) -> Result<Opened, AsrError> {
    let endpoint = config.endpoint(provider);
    connect_at(provider, config, language, &endpoint).await
}

// Endpoint override is internal for loopback protocol tests; API configuration
// only constructs the allowlisted provider WSS URLs in Config::endpoint.
pub(super) async fn connect_at(
    provider: Provider,
    config: Config,
    language: &str,
    endpoint: &str,
) -> Result<Opened, AsrError> {
    config.validate(provider)?;
    if !config.configured(provider) {
        return Err(AsrError::new(
            "external_asr_not_configured",
            "Configure external ASR credentials in Settings > Assistants first",
        ));
    }
    let task = uuid::Uuid::new_v4().to_string();
    let mut request = endpoint
        .into_client_request()
        .map_err(|_| transport_error())?;
    let credentials = match provider {
        Provider::Bailian => vec![("Authorization", format!("Bearer {}", config.api_key))],
        Provider::Volcengine if config.auth_mode == "app_token" => vec![
            ("X-Api-App-Key", config.app_id.clone()),
            ("X-Api-Access-Key", config.access_token.clone()),
        ],
        Provider::Volcengine => vec![("X-Api-Key", config.api_key.clone())],
    };
    for (name, value) in credentials {
        let mut header = HeaderValue::from_str(&value).map_err(|_| transport_error())?;
        header.set_sensitive(true);
        request.headers_mut().insert(name, header);
    }
    if provider == Provider::Volcengine {
        request
            .headers_mut()
            .insert("X-Api-Sequence", HeaderValue::from_static("-1"));
        request.headers_mut().insert(
            "X-Api-Resource-Id",
            HeaderValue::from_str(&config.resource_id).map_err(|_| transport_error())?,
        );
        request.headers_mut().insert(
            "X-Api-Connect-Id",
            HeaderValue::from_str(&task).map_err(|_| transport_error())?,
        );
        request.headers_mut().insert(
            "X-Api-Request-Id",
            HeaderValue::from_str(&task).map_err(|_| transport_error())?,
        );
    }
    let model = format!("{}:{}", provider.id(), config.model_id(provider));
    let codec = Codec { provider, task };
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let settings = WebSocketConfig::default()
            .max_message_size(Some(volcengine::MAX_REPLY_BYTES))
            .max_frame_size(Some(volcengine::MAX_REPLY_BYTES));
        let (mut socket, _) =
            tokio_tungstenite::connect_async_with_config(request, Some(settings), true)
                .await
                .map_err(|error| super::connection_error::classify(provider, error))?;
        let start = match provider {
            Provider::Bailian => bailian::start(&config, &codec.task, language),
            Provider::Volcengine => volcengine::start(&codec.task)?,
        };
        socket.send(start).await.map_err(|_| transport_error())?;
        // The optimized Volcengine stream has no task-started event and may
        // emit nothing until audio arrives. Waiting here would deadlock capture.
        if provider == Provider::Volcengine {
            return Ok(Opened {
                socket,
                codec,
                model,
            });
        }
        loop {
            let message = socket
                .next()
                .await
                .ok_or_else(transport_error)?
                .map_err(|_| transport_error())?;
            match codec.parse(message)? {
                (Event::Ready, false) => break,
                (Event::Ignore, false) => {}
                _ => {
                    return Err(AsrError::new(
                        "external_asr_protocol_error",
                        "ASR task did not acknowledge startup",
                    ));
                }
            }
        }
        Ok(Opened {
            socket,
            codec,
            model,
        })
    })
    .await
    .map_err(|_| {
        AsrError::new(
            "external_asr_start_timeout",
            "External ASR startup timed out",
        )
    })?
}

pub(super) async fn send(writer: &mut Writer, message: Message) -> Result<(), AsrError> {
    tokio::time::timeout(Duration::from_secs(5), writer.send(message))
        .await
        .map_err(|_| {
            AsrError::new(
                "external_asr_backpressure",
                "External ASR stopped accepting audio; recording was stopped",
            )
        })?
        .map_err(|_| transport_error())
}

pub(super) fn transport_error() -> AsrError {
    // Never forward HTTP bodies, request headers or vendor error messages: they
    // can echo credentials, audio text or private workspace identifiers.
    AsrError::new(
        "external_asr_connection_failed",
        "Could not connect to the ASR provider; check network access and provider configuration",
    )
}
