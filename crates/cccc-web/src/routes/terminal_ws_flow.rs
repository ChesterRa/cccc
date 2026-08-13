use axum::extract::ws::Message;
use serde::Deserialize;
use std::collections::VecDeque;

pub(super) const OUTPUT_ACK_V1: &str = "ack_v1";
const OUTPUT_WINDOW_BYTES: usize = 256 * 1024;

#[derive(Debug)]
struct PendingOutput {
    end_cursor: u64,
    bytes: usize,
}

pub(super) struct OutputFlow {
    enabled: bool,
    pending: VecDeque<PendingOutput>,
    pending_bytes: usize,
}

impl OutputFlow {
    pub(super) fn new(requested: Option<&str>) -> Self {
        Self {
            enabled: requested == Some(OUTPUT_ACK_V1),
            pending: VecDeque::new(),
            pending_bytes: 0,
        }
    }

    pub(super) fn protocol(&self) -> Option<&'static str> {
        self.enabled.then_some(OUTPUT_ACK_V1)
    }

    pub(super) const fn window_bytes(&self) -> usize {
        OUTPUT_WINDOW_BYTES
    }

    pub(super) fn can_send(&self, max_next_bytes: usize) -> bool {
        !self.enabled || self.pending_bytes.saturating_add(max_next_bytes) <= OUTPUT_WINDOW_BYTES
    }

    pub(super) fn record_send(&mut self, end_cursor: u64, bytes: usize) {
        if !self.enabled || bytes == 0 {
            return;
        }
        self.pending.push_back(PendingOutput { end_cursor, bytes });
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        debug_assert!(self.pending_bytes <= OUTPUT_WINDOW_BYTES);
    }

    pub(super) fn acknowledge(&mut self, cursor: u64) -> bool {
        if !self.enabled
            || self
                .pending
                .back()
                .is_none_or(|pending| cursor > pending.end_cursor)
        {
            return false;
        }
        while self
            .pending
            .front()
            .is_some_and(|pending| pending.end_cursor <= cursor)
        {
            if let Some(pending) = self.pending.pop_front() {
                self.pending_bytes = self.pending_bytes.saturating_sub(pending.bytes);
            }
        }
        true
    }
}

#[derive(Deserialize)]
struct OutputAck {
    cursor: u64,
}

pub(super) fn output_ack_cursor(message: &Message) -> Option<u64> {
    let Message::Binary(data) = message else {
        return None;
    };
    let (opcode, payload) = data.split_first()?;
    if *opcode != b'5' {
        return None;
    }
    serde_json::from_slice::<OutputAck>(payload)
        .ok()
        .map(|ack| ack.cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_flow_bounds_unparsed_output_and_releases_cumulatively() {
        let mut flow = OutputFlow::new(Some(OUTPUT_ACK_V1));
        for index in 1..=3 {
            flow.record_send(index * 65_536, 65_536);
        }
        assert!(flow.can_send(65_536));
        flow.record_send(4 * 65_536, 65_536);
        assert!(!flow.can_send(65_536));
        assert_eq!(flow.pending_bytes, OUTPUT_WINDOW_BYTES);

        assert!(flow.acknowledge(2 * 65_536));
        assert!(flow.can_send(65_536));
        assert_eq!(flow.pending_bytes, 2 * 65_536);
        assert!(!flow.acknowledge(9 * 65_536));
    }

    #[test]
    fn partial_first_page_never_overshoots_the_advertised_window() {
        let mut flow = OutputFlow::new(Some(OUTPUT_ACK_V1));
        flow.record_send(60_000, 60_000);
        for index in 1..=3 {
            flow.record_send(60_000 + index * 65_536, 65_536);
        }

        assert_eq!(flow.pending_bytes, 256_608);
        assert!(!flow.can_send(65_536));
        assert!(flow.pending_bytes <= flow.window_bytes());
    }

    #[test]
    fn legacy_flow_is_unrestricted_and_ignores_ack_state() {
        let mut flow = OutputFlow::new(None);
        flow.record_send(1_000_000, 1_000_000);
        assert!(flow.can_send(1_000_000));
        assert_eq!(flow.protocol(), None);
        assert!(!flow.acknowledge(1_000_000));
    }

    #[test]
    fn parses_only_output_ack_frames() {
        let ack = Message::Binary(b"5{\"cursor\":42}".to_vec().into());
        let input = Message::Binary(b"0hello".to_vec().into());
        assert_eq!(output_ack_cursor(&ack), Some(42));
        assert_eq!(output_ack_cursor(&input), None);
    }
}
