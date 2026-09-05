//! Inferred repaint frames for transcript browsing, separate from live snapshots.
use super::{MAX_SCREEN_ROWS, Screen};
use std::collections::VecDeque;

const MAX_FRAME_BYTES: usize = 50_000_000;

pub(crate) fn render(text: &str, compact: bool) -> String {
    let mut screen = Screen {
        history: Some(Frames::default()),
        ..Screen::default()
    };
    screen.render(text);
    screen.checkpoint();
    let frames = screen
        .history
        .expect("history renderer initializes frame storage");
    let mut output = if frames.truncated {
        vec!["[Earlier rendered frames omitted: display limit reached]".to_owned()]
    } else {
        Vec::new()
    };
    output.extend(frames.items.into_iter().map(|frame| {
        if compact {
            super::compact_lines(frame.lines().map(str::to_owned).collect()).join("\n")
        } else {
            frame
        }
    }));
    output.join("\n\n")
}

#[derive(Default)]
pub(super) struct Frames {
    items: VecDeque<String>,
    scrollback: VecDeque<String>,
    bytes: usize,
    truncated: bool,
}

impl Screen {
    pub(super) fn checkpoint(&mut self) {
        if !self.dirty || self.history.is_none() {
            return;
        }
        self.dirty = false;
        let screen = self.text(false);
        let frames = self.history.as_mut().expect("history mode was checked");
        let mut frame = String::new();
        for chunk in frames.scrollback.drain(..) {
            frames.bytes -= chunk.len();
            frame.push_str(&chunk);
        }
        frame.push_str(&screen);
        if frame.len() + 2 > MAX_FRAME_BYTES {
            let mut start = frame.len() + 2 - MAX_FRAME_BYTES;
            while !frame.is_char_boundary(start) {
                start += 1;
            }
            frame.drain(..start);
            frames.truncated = true;
        }
        if frame.is_empty() || frames.items.back() == Some(&frame) {
            return;
        }
        frames.bytes += frame.len() + 2;
        frames.items.push_back(frame);
        frames.trim();
    }

    pub(super) fn line_feed(&mut self) {
        if self.row == MAX_SCREEN_ROWS - 1 && self.history.is_some() {
            // Cursor coordinates stay screen-relative; lines leaving the
            // bounded screen belong to history, not to the last screen row.
            let row = self.rows.pop_front().unwrap_or_default();
            let text = row.iter().filter(|c| **c != '\0').collect::<String>();
            let line = text.trim_end().to_owned();
            let frames = self.history.as_mut().expect("history mode was checked");
            frames.bytes += line.len() + 1;
            // Chunk the scrollback: newline-only input must not allocate one
            // String/queue entry per line across a 50 MB raw transcript.
            if frames
                .scrollback
                .back()
                .is_none_or(|chunk| chunk.len() >= 64_000)
            {
                frames.scrollback.push_back(String::new());
            }
            let chunk = frames
                .scrollback
                .back_mut()
                .expect("scrollback chunk was initialized");
            chunk.push_str(&line);
            chunk.push('\n');
            frames.trim();
            self.saved_cursor.0 = self.saved_cursor.0.saturating_sub(1);
        } else {
            self.row = super::bounded_row(self.row.saturating_add(1));
        }
        self.col = 0;
        self.ensure_row();
    }
}

impl Frames {
    fn trim(&mut self) {
        while self.bytes > MAX_FRAME_BYTES {
            if let Some(frame) = self.items.pop_front() {
                self.bytes -= frame.len() + 2;
            } else if let Some(chunk) = self.scrollback.pop_front() {
                self.bytes -= chunk.len();
            } else {
                break;
            }
            self.truncated = true;
        }
    }
}

pub(super) fn is_redraw(command: char, params: &[usize], cursor: (usize, usize)) -> bool {
    let first = params.first().copied().unwrap_or(1).max(1) - 1;
    match command {
        'H' | 'f' => (first, params.get(1).copied().unwrap_or(1).max(1) - 1) <= cursor,
        'G' => first < cursor.1,
        'd' => first < cursor.0,
        'A' | 'D' | 'J' | 'K' | 'u' => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn append_only_history_keeps_every_line_beyond_screen_capacity() {
        let raw = (0..6000).map(|n| format!("line {n}\n")).collect::<String>();
        let text = render(&raw, false);
        let expected = raw.trim_end();
        assert_eq!(text, expected);
        assert_eq!(render(&raw, true), expected);
        let repainted = render(&format!("{raw}\x1b[H\x1b[2Jnew frame"), false);
        assert!(repainted.starts_with(expected));
        assert!(repainted.ends_with("new frame"));
    }

    #[test]
    fn keeps_frames_before_cursor_repaint_clear_and_alternate_screen_exit() {
        for raw in [
            "\x1b[1;1Hold frame\x1b[1;1H\x1b[2Knew frame",
            "old frame\x1b[2Jnew frame",
            "old frame\rnew frame",
            "old frame\x1b[?1049hnew frame\x1b[?1049l",
        ] {
            assert_eq!(render(raw, false), "old frame\n\nnew frame");
        }
    }

    #[test]
    fn leaves_append_only_output_and_colors_in_one_frame() {
        assert_eq!(render("a\r\n\x1b[31mb\x1b[0m\nc", false), "a\nb\nc");
    }

    #[test]
    fn repeated_identical_repaints_do_not_duplicate_frames() {
        assert_eq!(render("same\r\x1b[2Ksame\r\x1b[2Ksame", false), "same");
    }
}
