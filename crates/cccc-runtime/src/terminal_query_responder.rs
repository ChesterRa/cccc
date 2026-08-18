const MAX_OSC_QUERY_BYTES: usize = 4 * 1024;
const MAX_CSI_QUERY_BYTES: usize = 64;
const MAX_RESPONSES_PER_READ: usize = 64;
const TERMINAL_FOREGROUND: &[u8] = b"\x1b]10;rgb:e2e2/e8e8/f0f0\x1b\\";
const TERMINAL_BACKGROUND: &[u8] = b"\x1b]11;rgb:0f0f/1717/2a2a\x1b\\";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryState {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
    DiscardOsc,
    DiscardOscEscape,
}

#[derive(Debug)]
pub(crate) struct TerminalQueryResponder {
    state: QueryState,
    osc_payload: Vec<u8>,
    csi_payload: Vec<u8>,
}

pub(crate) struct TerminalQueryResponse {
    pub(crate) end_offset: usize,
    pub(crate) reply: TerminalQueryReply,
}

pub(crate) enum TerminalQueryReply {
    Bytes(Vec<u8>),
    PrivateCursorPosition,
}

impl Default for TerminalQueryResponder {
    fn default() -> Self {
        Self {
            state: QueryState::Ground,
            osc_payload: Vec::new(),
            csi_payload: Vec::new(),
        }
    }
}

impl TerminalQueryResponder {
    pub(crate) fn process(&mut self, data: &[u8]) -> Vec<TerminalQueryResponse> {
        let mut responses = Vec::new();
        for (index, &byte) in data.iter().enumerate() {
            self.advance(byte, index + 1, &mut responses);
        }
        responses
    }

    fn advance(&mut self, byte: u8, end_offset: usize, responses: &mut Vec<TerminalQueryResponse>) {
        match self.state {
            QueryState::Ground => match byte {
                0x1b => self.state = QueryState::Escape,
                0x9b => self.begin_csi(),
                0x9d => self.begin_osc(),
                _ => {}
            },
            QueryState::Escape => match byte {
                b'[' => self.begin_csi(),
                b']' => self.begin_osc(),
                0x1b => {}
                0x9d => self.begin_osc(),
                _ => self.state = QueryState::Ground,
            },
            QueryState::Csi => self.advance_csi(byte, end_offset, responses),
            QueryState::Osc => match byte {
                0x07 | 0x9c => self.finish_osc(end_offset, responses),
                0x1b => self.state = QueryState::OscEscape,
                _ => self.push_osc(byte),
            },
            QueryState::OscEscape => {
                if byte == b'\\' {
                    self.finish_osc(end_offset, responses);
                } else {
                    self.push_osc(0x1b);
                    if self.state == QueryState::DiscardOsc {
                        return;
                    }
                    if byte == 0x1b {
                        self.state = QueryState::OscEscape;
                    } else {
                        self.push_osc(byte);
                    }
                }
            }
            QueryState::DiscardOsc => match byte {
                0x07 | 0x9c => self.reset(),
                0x1b => self.state = QueryState::DiscardOscEscape,
                _ => {}
            },
            QueryState::DiscardOscEscape => {
                self.state = if byte == b'\\' {
                    QueryState::Ground
                } else if byte == 0x1b {
                    QueryState::DiscardOscEscape
                } else {
                    QueryState::DiscardOsc
                };
            }
        }
    }

    fn begin_osc(&mut self) {
        self.osc_payload.clear();
        self.state = QueryState::Osc;
    }

    fn begin_csi(&mut self) {
        self.csi_payload.clear();
        self.state = QueryState::Csi;
    }

    fn advance_csi(
        &mut self,
        byte: u8,
        end_offset: usize,
        responses: &mut Vec<TerminalQueryResponse>,
    ) {
        if byte == 0x1b {
            self.csi_payload.clear();
            self.state = QueryState::Escape;
            return;
        }
        if matches!(byte, 0x18 | 0x1a) {
            self.csi_payload.clear();
            self.state = QueryState::Ground;
            return;
        }
        if self.csi_payload.len() >= MAX_CSI_QUERY_BYTES {
            self.csi_payload.clear();
            self.state = QueryState::Ground;
            return;
        }
        self.csi_payload.push(byte);
        if (0x40..=0x7e).contains(&byte) {
            if self.csi_payload == b"?6n" && responses.len() < MAX_RESPONSES_PER_READ {
                responses.push(TerminalQueryResponse {
                    end_offset,
                    reply: TerminalQueryReply::PrivateCursorPosition,
                });
            }
            self.csi_payload.clear();
            self.state = QueryState::Ground;
        }
    }

    fn push_osc(&mut self, byte: u8) {
        if self.osc_payload.len() >= MAX_OSC_QUERY_BYTES {
            self.osc_payload.clear();
            self.state = QueryState::DiscardOsc;
            return;
        }
        self.osc_payload.push(byte);
        self.state = QueryState::Osc;
    }

    fn finish_osc(&mut self, end_offset: usize, responses: &mut Vec<TerminalQueryResponse>) {
        append_color_query_responses(&self.osc_payload, end_offset, responses);
        self.reset();
    }

    fn reset(&mut self) {
        self.osc_payload.clear();
        self.state = QueryState::Ground;
    }
}

fn append_color_query_responses(
    payload: &[u8],
    end_offset: usize,
    responses: &mut Vec<TerminalQueryResponse>,
) {
    let mut fields = payload.split(|byte| *byte == b';');
    let Some(mut color) = fields
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.parse::<u8>().ok())
    else {
        return;
    };
    for value in fields {
        if responses.len() >= MAX_RESPONSES_PER_READ {
            return;
        }
        if value == b"?" {
            let data = match color {
                10 => Some(TERMINAL_FOREGROUND.to_vec()),
                11 => Some(TERMINAL_BACKGROUND.to_vec()),
                _ => None,
            };
            if let Some(data) = data {
                responses.push(TerminalQueryResponse {
                    end_offset,
                    reply: TerminalQueryReply::Bytes(data),
                });
            }
        }
        color = color.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TERMINAL_BACKGROUND, TERMINAL_FOREGROUND, TerminalQueryReply, TerminalQueryResponder,
    };

    fn response_data(responses: Vec<super::TerminalQueryResponse>) -> Vec<Vec<u8>> {
        responses
            .into_iter()
            .filter_map(|response| match response.reply {
                TerminalQueryReply::Bytes(data) => Some(data),
                TerminalQueryReply::PrivateCursorPosition => None,
            })
            .collect()
    }

    #[test]
    fn answers_split_foreground_and_background_queries() {
        let mut responder = TerminalQueryResponder::default();

        assert!(responder.process(b"\x1b]10;?").is_empty());
        assert_eq!(
            response_data(responder.process(b";?\x1b\\")),
            [TERMINAL_FOREGROUND.to_vec(), TERMINAL_BACKGROUND.to_vec()]
        );
    }

    #[test]
    fn supports_bell_and_c1_osc_terminators() {
        let mut responder = TerminalQueryResponder::default();

        assert_eq!(
            response_data(responder.process(b"\x1b]11;?\x07\x9d10;?\x9c")),
            [TERMINAL_BACKGROUND.to_vec(), TERMINAL_FOREGROUND.to_vec()]
        );
    }

    #[test]
    fn ignores_color_updates_and_unrelated_osc_sequences() {
        let mut responder = TerminalQueryResponder::default();

        assert!(
            responder
                .process(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]2;title\x07")
                .is_empty()
        );
    }

    #[test]
    fn detects_a_split_private_cursor_position_query() {
        let mut responder = TerminalQueryResponder::default();

        assert!(responder.process(b"\x1b[?6").is_empty());
        let responses = responder.process(b"n");
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            &responses[0].reply,
            TerminalQueryReply::PrivateCursorPosition
        ));
    }
}
