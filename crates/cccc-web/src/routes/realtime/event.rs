use axum::response::sse::Event;
use futures_util::Stream;
use serde::Serialize;
use serde_json::Value;
use std::{convert::Infallible, pin::Pin};

pub(crate) type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, Infallible>> + Send>>;

#[derive(Serialize)]
pub(crate) struct StreamEvent {
    pub event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub data: Value,
}

impl StreamEvent {
    pub fn new(event: &'static str, data: impl Serialize) -> Self {
        Self {
            event,
            id: None,
            data: serde_json::to_value(data).unwrap_or_default(),
        }
    }
    pub fn with_id(mut self, id: String) -> Self {
        self.id = Some(id);
        self
    }
    pub fn connected() -> Self {
        Self::new("connected", Value::Null)
    }
    pub fn error(code: &str, message: impl AsRef<str>) -> Self {
        Self::new(
            "error",
            serde_json::json!({"ok":false,"error":{"code":code,"message":message.as_ref()}}),
        )
    }
    pub fn into_sse(self) -> Event {
        if self.event == "connected" {
            return Event::default()
                .comment("connected")
                .retry(std::time::Duration::from_secs(1));
        }
        let mut event = Event::default()
            .event(self.event)
            .json_data(self.data)
            .unwrap_or_default();
        if let Some(id) = self.id {
            event = event.id(id);
        }
        event
    }
}
