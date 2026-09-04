use std::collections::VecDeque;
use std::io;

const MAX_PENDING_NATIVE_INPUTS: usize = 64;

#[derive(Debug)]
pub(super) struct PendingNativeInput {
    pub(super) delegation_id: String,
    pub(super) expected_text: String,
    observed_text: String,
}

pub(super) fn register(
    pending: &mut VecDeque<PendingNativeInput>,
    delegation_id: String,
    text: String,
) -> io::Result<()> {
    if let Some(existing) = pending
        .iter()
        .find(|item| item.delegation_id == delegation_id)
    {
        return if existing.expected_text == text {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Voice delegation id was reused with different input",
            ))
        };
    }
    if pending.len() >= MAX_PENDING_NATIVE_INPUTS {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "managed Runtime native input correlation capacity is full",
        ));
    }
    pending.push_back(PendingNativeInput {
        delegation_id,
        expected_text: text,
        observed_text: String::new(),
    });
    Ok(())
}

pub(super) fn forget(pending: &mut VecDeque<PendingNativeInput>, delegation_id: &str) {
    if let Some(index) = pending
        .iter()
        .position(|item| item.delegation_id == delegation_id)
    {
        pending.remove(index);
    }
}

/// Match an authoritative Runtime echo against the oldest CCCC-submitted native input.
///
/// ACP providers may emit either incremental chunks or a cumulative replacement. A mismatch is
/// left untouched because it can be unrelated human terminal input that raced ahead of the CCCC
/// write; only the exact registered payload consumes ownership.
pub(super) fn observe(pending: &mut VecDeque<PendingNativeInput>, text: &str) -> Option<String> {
    let item = pending.front_mut()?;
    let combined = format!("{}{text}", item.observed_text);
    if item.expected_text.starts_with(&combined) {
        item.observed_text = combined;
    } else if item.expected_text.starts_with(text) {
        item.observed_text = text.to_owned();
    } else {
        return None;
    }
    if item.observed_text != item.expected_text {
        return None;
    }
    pending.pop_front().map(|item| item.delegation_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_native_input_correlation_is_fifo_and_chunk_safe() {
        let mut pending = VecDeque::new();
        register(&mut pending, "d1".into(), "first prompt".into()).expect("register first");
        register(&mut pending, "d2".into(), "second".into()).expect("register second");

        assert_eq!(observe(&mut pending, "unrelated terminal text"), None);
        assert_eq!(observe(&mut pending, "first "), None);
        assert_eq!(observe(&mut pending, "prompt"), Some("d1".into()));
        assert_eq!(observe(&mut pending, "second"), Some("d2".into()));
        assert!(pending.is_empty());
    }

    #[test]
    fn duplicate_registration_is_idempotent_but_cannot_change_text() {
        let mut pending = VecDeque::new();
        register(&mut pending, "d1".into(), "same".into()).expect("register");
        register(&mut pending, "d1".into(), "same".into()).expect("idempotent replay");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            register(&mut pending, "d1".into(), "different".into())
                .expect_err("changed replay")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
