mod history;
pub(super) use history::render as render_history;

use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;
type Rows = VecDeque<Vec<char>>;

const MAX_SCREEN_ROWS: usize = 4_096;
const MAX_SCREEN_COLS: usize = 1_024;

pub(super) fn render(text: &str, compact: bool) -> String {
    let mut screen = Screen::default();
    screen.render(text);
    screen.text(compact)
}

#[derive(Default)]
struct Screen {
    history: Option<history::Frames>,
    dirty: bool,
    rows: Rows,
    row: usize,
    col: usize,
    saved_cursor: (usize, usize),
    main_screen: Option<(Rows, usize, usize)>,
}

impl Screen {
    fn render(&mut self, text: &str) {
        let input: Vec<char> = text.replace("\r\n", "\n").chars().collect();
        let mut index = 0;
        while index < input.len() {
            match input[index] {
                '\u{1b}' => index = self.escape(&input, index),
                '\n' => {
                    self.line_feed();
                    index += 1;
                }
                '\r' => {
                    self.checkpoint();
                    self.col = 0;
                    index += 1;
                }
                '\u{8}' => {
                    self.checkpoint();
                    self.col = self.col.saturating_sub(1);
                    index += 1;
                }
                character if character.is_control() => index += 1,
                character => {
                    self.set_char(character);
                    let width = character.width().unwrap_or(1).max(1);
                    if width > 1 {
                        self.ensure_col(self.col.saturating_add(width - 1));
                        for offset in 1..width {
                            self.rows[self.row][self.col + offset] = '\0';
                        }
                    }
                    self.col = bounded_col(self.col.saturating_add(width));
                    index += 1;
                }
            }
        }
    }

    fn escape(&mut self, input: &[char], index: usize) -> usize {
        let Some(next) = input.get(index + 1) else {
            return input.len();
        };
        match next {
            ']' | 'P' | 'X' | '^' | '_' => skip_string_sequence(input, index + 2),
            '[' => self.csi(input, index + 2),
            '7' => {
                self.saved_cursor = (self.row, self.col);
                index + 2
            }
            '8' => {
                self.checkpoint();
                (self.row, self.col) = self.saved_cursor;
                self.ensure_row();
                index + 2
            }
            _ => index + 2,
        }
    }

    fn csi(&mut self, input: &[char], start: usize) -> usize {
        let private = input.get(start) == Some(&'?');
        let params_start = start + usize::from(private);
        let mut final_index = params_start;
        while final_index < input.len() && !('@'..='~').contains(&input[final_index]) {
            final_index += 1;
        }
        if final_index >= input.len() {
            return input.len();
        }
        let params: String = input[params_start..final_index].iter().collect();
        let params = parse_params(&params);
        let final_char = input[final_index];

        if private {
            self.private_mode(final_char, &params);
            return final_index + 1;
        }

        if history::is_redraw(final_char, &params, (self.row, self.col)) {
            self.checkpoint();
        }
        let first = || params.first().copied().unwrap_or(1).max(1);
        match final_char {
            'H' | 'f' => {
                self.row = bounded_row(params.first().copied().unwrap_or(1).max(1) - 1);
                self.col = bounded_col(params.get(1).copied().unwrap_or(1).max(1) - 1);
                self.ensure_row();
            }
            'A' => self.row = self.row.saturating_sub(first()),
            'B' => {
                self.row = bounded_row(self.row.saturating_add(first()));
                self.ensure_row();
            }
            'C' => self.col = bounded_col(self.col.saturating_add(first())),
            'D' => self.col = self.col.saturating_sub(first()),
            'G' => self.col = bounded_col(first() - 1),
            'd' => {
                self.row = bounded_row(first() - 1);
                self.ensure_row();
            }
            'J' => self.erase_display(params.first().copied().unwrap_or(0)),
            'K' => self.erase_line(params.first().copied().unwrap_or(0)),
            's' => self.saved_cursor = (self.row, self.col),
            'u' => {
                (self.row, self.col) = self.saved_cursor;
                self.ensure_row();
            }
            _ => {}
        }
        final_index + 1
    }

    fn private_mode(&mut self, final_char: char, params: &[usize]) {
        if !params.contains(&1049) {
            return;
        }
        self.checkpoint();
        match final_char {
            'h' if self.main_screen.is_none() => {
                self.main_screen = Some((self.rows.clone(), self.row, self.col));
                self.rows.clear();
                self.row = 0;
                self.col = 0;
                self.saved_cursor = (0, 0);
            }
            'l' => {
                if let Some((rows, row, col)) = self.main_screen.take() {
                    self.rows = rows;
                    self.row = row;
                    self.col = col;
                    self.saved_cursor = (row, col);
                }
            }
            _ => {}
        }
    }

    fn ensure_row(&mut self) {
        while self.rows.len() <= self.row {
            self.rows.push_back(Vec::new());
        }
    }

    fn ensure_col(&mut self, col: usize) {
        self.ensure_row();
        if self.rows[self.row].len() <= col {
            self.rows[self.row].resize(col + 1, ' ');
        }
    }

    fn set_char(&mut self, character: char) {
        self.dirty = true;
        self.ensure_col(self.col);
        self.rows[self.row][self.col] = character;
    }

    fn erase_line(&mut self, mode: usize) {
        self.ensure_row();
        match mode {
            2 => self.rows[self.row].clear(),
            1 => {
                self.ensure_col(self.col);
                for cell in &mut self.rows[self.row][..=self.col] {
                    *cell = ' ';
                }
            }
            _ => {
                let line = &mut self.rows[self.row];
                if self.col < line.len() {
                    line.truncate(self.col);
                }
            }
        }
    }

    fn erase_display(&mut self, mode: usize) {
        match mode {
            2 => {
                self.rows.clear();
                self.row = 0;
                self.col = 0;
                self.ensure_row();
            }
            1 => {
                for row in self.rows.iter_mut().take(self.row) {
                    row.clear();
                }
                self.erase_line(1);
            }
            _ => {
                self.erase_line(0);
                self.rows.truncate(self.row + 1);
            }
        }
    }

    fn text(&self, compact: bool) -> String {
        let mut lines: Vec<String> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|character| **character != '\0')
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if compact {
            lines = compact_lines(lines);
        }
        lines.join("\n")
    }
}

fn bounded_row(value: usize) -> usize {
    value.min(MAX_SCREEN_ROWS - 1)
}

fn bounded_col(value: usize) -> usize {
    value.min(MAX_SCREEN_COLS - 1)
}

fn parse_params(value: &str) -> Vec<usize> {
    value
        .split(';')
        .filter_map(|part| part.trim().parse().ok())
        .collect()
}

fn skip_string_sequence(input: &[char], mut index: usize) -> usize {
    while index < input.len() {
        if input[index] == '\u{7}' {
            return index + 1;
        }
        if input[index] == '\u{1b}' && input.get(index + 1) == Some(&'\\') {
            return index + 2;
        }
        index += 1;
    }
    input.len()
}

fn compact_lines(lines: Vec<String>) -> Vec<String> {
    let mut output = Vec::with_capacity(lines.len());
    for line in lines {
        let duplicate = output
            .last()
            .is_some_and(|previous: &String| normalized_line(previous) == normalized_line(&line));
        if !duplicate {
            output.push(line);
        }
    }
    output
}

fn normalized_line(line: &str) -> &str {
    line.trim_end()
}

#[cfg(test)]
mod tests;
