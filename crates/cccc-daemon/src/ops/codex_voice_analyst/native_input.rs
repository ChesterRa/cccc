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

/// Match an authoritative Runtime echo to its exact registered input.
///
/// Missing or reordered echoes cannot block independently confirmed later inputs. Identical
/// payloads retain registration order. Partial candidates are cleared after a complete echo so
/// a later unrelated suffix cannot consume an older input.
pub(super) fn observe(pending: &mut VecDeque<PendingNativeInput>, text: &str) -> Option<String> {
    // A full echo is stronger evidence than another input's accumulated prefix.
    let index = pending
        .iter()
        .position(|item| item.expected_text == text)
        .or_else(|| {
            pending.iter_mut().enumerate().find_map(|(index, item)| {
                let combined = format!("{}{text}", item.observed_text);
                if item.expected_text.starts_with(&combined) {
                    item.observed_text = combined;
                } else if item.expected_text.starts_with(text) {
                    item.observed_text = text.to_owned();
                } else {
                    item.observed_text.clear();
                    return None;
                }
                (item.observed_text == item.expected_text).then_some(index)
            })
        })?;
    let matched = pending.remove(index)?;
    for item in pending {
        item.observed_text.clear();
    }
    Some(matched.delegation_id)
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

    #[test]
    fn one_missing_echo_does_not_block_a_hundred_later_inputs() {
        let mut pending = VecDeque::new();
        register(&mut pending, "missing".into(), "missing input".into())
            .expect("register native input");
        for index in 0..100 {
            let id = format!("input-{index}");
            let text = format!("confirmed input {index}");
            register(&mut pending, id.clone(), text.clone()).expect("register native input");
            assert_eq!(observe(&mut pending, &text), Some(id));
            assert_eq!(pending.len(), 1, "retain only the unconfirmed input");
        }
        assert_eq!(pending[0].delegation_id, "missing");
        assert_eq!(
            observe(&mut pending, "missing input"),
            Some("missing".into())
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn later_fragmented_echo_matches_without_leaving_an_old_partial_claim() {
        let mut pending = VecDeque::new();
        register(&mut pending, "missing".into(), "shared missing".into())
            .expect("register native input");
        register(&mut pending, "later".into(), "shared confirmed".into())
            .expect("register native input");
        assert_eq!(observe(&mut pending, "shared "), None);
        assert_eq!(observe(&mut pending, "confirmed"), Some("later".into()));
        assert_eq!(observe(&mut pending, "missing"), None);
        assert_eq!(pending.len(), 1);
        assert_eq!(
            observe(&mut pending, "shared missing"),
            Some("missing".into())
        );
    }

    #[test]
    fn complete_echo_takes_precedence_over_another_inputs_partial_suffix() {
        let mut pending = VecDeque::new();
        register(&mut pending, "missing".into(), "prefix suffix".into())
            .expect("register native input");
        register(&mut pending, "later".into(), "suffix".into()).expect("register native input");
        assert_eq!(observe(&mut pending, "prefix "), None);
        assert_eq!(observe(&mut pending, "suffix"), Some("later".into()));
        assert_eq!(pending[0].delegation_id, "missing");
        assert_eq!(
            observe(&mut pending, "prefix suffix"),
            Some("missing".into())
        );
    }

    #[test]
    fn identical_inputs_keep_their_registration_order() {
        let mut pending = VecDeque::new();
        register(&mut pending, "first".into(), "same".into()).expect("register native input");
        register(&mut pending, "second".into(), "same".into()).expect("register native input");
        assert_eq!(observe(&mut pending, "same"), Some("first".into()));
        assert_eq!(observe(&mut pending, "same"), Some("second".into()));
    }
}
