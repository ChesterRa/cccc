use serde_json::{Value, json};
use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub(super) const EVENT_QUEUE_CAPACITY: usize = 8;
pub(super) const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_BUFFERED_ITEMS: usize = 1_024;

pub(super) enum BoundedLine {
    Data(Vec<u8>),
    TooLong,
}

pub(super) async fn read_bounded_line<R>(reader: &mut R) -> io::Result<Option<BoundedLine>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if line.is_empty() && !too_long {
                Ok(None)
            } else if too_long {
                Ok(Some(BoundedLine::TooLong))
            } else {
                Ok(Some(BoundedLine::Data(line)))
            };
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let payload_len = newline.unwrap_or(available.len());
        if !too_long {
            if line.len().saturating_add(payload_len) > MAX_EVENT_BYTES {
                line.clear();
                too_long = true;
            } else {
                line.extend_from_slice(&available[..payload_len]);
            }
        }
        let consumed = newline.map_or(available.len(), |index| index + 1);
        reader.consume(consumed);

        if newline.is_some() {
            if too_long {
                return Ok(Some(BoundedLine::TooLong));
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(BoundedLine::Data(line)));
        }
    }
}

pub(super) struct OutputBuffer {
    items: Vec<Value>,
    remaining_chars: usize,
    truncated: bool,
}

impl OutputBuffer {
    pub(super) fn new(max_output_tokens: usize) -> Self {
        Self {
            items: Vec::new(),
            remaining_chars: max_output_tokens.saturating_mul(4).max(1),
            truncated: false,
        }
    }

    pub(super) fn push(&mut self, item: Value) {
        if self.remaining_chars == 0 || self.items.len() >= MAX_BUFFERED_ITEMS {
            self.truncated = true;
            return;
        }
        let text = content_text(&item);
        let length = text.chars().count();
        let cost = length.max(1);
        if cost > self.remaining_chars {
            let prefix = text.chars().take(self.remaining_chars).collect::<String>();
            self.items
                .push(json!({"type":"text","text":format!("{prefix}\n[truncated]")}));
            self.remaining_chars = 0;
            self.truncated = true;
            return;
        }
        self.items.push(item);
        self.remaining_chars -= cost;
    }

    pub(super) fn mark_truncated(&mut self) {
        self.truncated = true;
    }

    pub(super) fn into_parts(self) -> (Vec<Value>, bool) {
        (self.items, self.truncated)
    }
}

pub(super) fn content_text(item: &Value) -> String {
    if item.get("type").and_then(Value::as_str).unwrap_or("text") == "text" {
        item.get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    } else {
        serde_json::to_string(item).unwrap_or_else(|_| item.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[test]
    fn output_buffer_bounds_empty_and_oversized_items_before_storage() {
        let mut output = OutputBuffer::new(2);
        for _ in 0..MAX_BUFFERED_ITEMS + 10 {
            output.push(json!({"type":"text","text":""}));
        }
        let (items, truncated) = output.into_parts();
        assert_eq!(items.len(), 8);
        assert!(truncated);

        let mut output = OutputBuffer::new(2);
        output.push(json!({"type":"text","text":"123456789"}));
        let (items, truncated) = output.into_parts();
        assert_eq!(content_text(&items[0]), "12345678\n[truncated]");
        assert!(truncated);
    }

    #[tokio::test]
    async fn line_reader_discards_oversized_protocol_frames() {
        let mut input = vec![b'x'; MAX_EVENT_BYTES + 1];
        input.extend_from_slice(b"\nok\n");
        let mut reader = BufReader::new(input.as_slice());

        assert!(matches!(
            read_bounded_line(&mut reader)
                .await
                .expect("oversized line"),
            Some(BoundedLine::TooLong)
        ));
        match read_bounded_line(&mut reader).await.expect("next line") {
            Some(BoundedLine::Data(line)) => assert_eq!(line, b"ok"),
            _ => panic!("expected the line after the oversized frame"),
        }
    }
}
