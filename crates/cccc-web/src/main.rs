#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
use anyhow::Result;
use cccc_core::HomeLayout;

#[tokio::main]
async fn main() -> Result<()> {
    cccc_core::build_info::initialize(env!("CCCC_SOURCE_ID"));
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(raw_handshake_guard())
        .with(tracing_subscriber::fmt::layer())
        .init();
    let host = std::env::var("CCCC_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("CCCC_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8848);
    cccc_web::serve(HomeLayout::resolve()?, &host, port).await?;
    Ok(())
}

/// tungstenite traces the raw client handshake, which carries the device relay's
/// Authorization header. This filter is independent of RUST_LOG, so no more
/// specific environment directive can re-enable those records.
fn raw_handshake_guard()
-> tracing_subscriber::filter::FilterFn<impl Fn(&tracing::Metadata<'_>) -> bool> {
    tracing_subscriber::filter::filter_fn(|metadata| {
        let target = metadata.target();
        let handshake_crate = ["tungstenite", "tokio_tungstenite"].iter().any(|name| {
            target == *name
                || target
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with("::"))
        });
        !handshake_crate || *metadata.level() <= tracing::Level::INFO
    })
}

#[cfg(test)]
mod tests {
    use super::raw_handshake_guard;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::prelude::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn handshake_traces_stay_out_even_when_the_environment_asks_for_them() {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                "trace,tungstenite::handshake::client=trace,tokio_tungstenite=trace",
            ))
            .with(raw_handshake_guard())
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(move || writer.clone()),
            );
        tracing::subscriber::with_default(subscriber, || {
            tracing::trace!(target: "tungstenite::handshake::client", "Request: Authorization: Bearer fixture-secret");
            tracing::debug!(target: "tokio_tungstenite", "fixture-secret debug");
            tracing::info!(target: "tungstenite::protocol", "handshake info stays");
            tracing::trace!(target: "tungstenite_like", "other crate trace stays");
        });
        let output = String::from_utf8(captured.0.lock().expect("capture").clone()).expect("utf8");
        assert!(!output.contains("fixture-secret"), "{output}");
        assert!(output.contains("handshake info stays"), "{output}");
        assert!(output.contains("other crate trace stays"), "{output}");
    }
}
