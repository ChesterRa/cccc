use super::{ActorRuntime, AnalystSession, ManagedProtocol};
use sha2::{Digest, Sha256};
use std::io;
use std::sync::{Mutex, Weak};
use tokio::sync::oneshot;

struct ExpectedInput {
    digest: [u8; 32],
    received: oneshot::Sender<()>,
}

/// Owned by the authenticated event stream of one provider session. Losing
/// that stream closes its outstanding receipt even if the ACP client survives.
#[derive(Default)]
pub(super) struct KiloUserMessages(Mutex<Option<ExpectedInput>>);

impl KiloUserMessages {
    fn expect(&self, text: &str) -> io::Result<oneshot::Receiver<()>> {
        let text = text.trim();
        if text.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty Kilo input",
            ));
        }
        let mut slot = self.0.lock().expect("Kilo input receipt lock");
        if slot
            .as_ref()
            .is_some_and(|input| !input.received.is_closed())
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Kilo input receipt already pending",
            ));
        }
        let (received, receiver) = oneshot::channel();
        // Kilo's paste handler normalizes CRLF and bare CR to LF before storage.
        let normalized;
        let stored = if text.contains('\r') {
            normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            normalized.as_str()
        } else {
            text
        };
        *slot = Some(ExpectedInput {
            digest: Sha256::digest(stored.as_bytes()).into(),
            received,
        });
        Ok(receiver)
    }

    pub(super) fn observe(&self, text: &str) {
        let mut slot = self.0.lock().expect("Kilo input receipt lock");
        if let Some(input) = &*slot {
            if input.received.is_closed() {
                *slot = None;
            } else if input.digest == <[u8; 32]>::from(Sha256::digest(text.as_bytes())) {
                let input = slot.take().expect("matched Kilo input");
                let _ = input.received.send(());
            }
        }
    }
}

impl AnalystSession {
    /// Arm before writing to the viewer. Dropping the receiver allows the next
    /// attempt to register immediately, including after a write error or timeout.
    pub(crate) fn expect_kilo_user_message(&self, text: &str) -> io::Result<oneshot::Receiver<()>> {
        let ManagedProtocol::Acp(protocol) = &self.protocol else {
            return Err(io::ErrorKind::Unsupported.into());
        };
        if self.runtime != ActorRuntime::Kilo {
            return Err(io::ErrorKind::Unsupported.into());
        }
        protocol
            .kilo_input
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Kilo input observer disconnected",
                )
            })?
            .expect(text)
    }
}
