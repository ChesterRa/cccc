//! Display helpers for text the interface did not write itself.

use std::path::{MAIN_SEPARATOR, Path};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

const CUT_MARK: &str = "...";

/// Make outside text safe for one screen row. Session titles and paths come
/// from other programs: a control character could move the cursor or recolor
/// the screen, and a line break would split the row.
pub(super) fn one_line(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| match ch {
            '\t' | '\n' | '\r' => Some(' '),
            ch if ch.is_control() || is_bidi_control(ch) => None,
            ch => Some(ch),
        })
        .collect()
}

// Explicit direction overrides can make a row read in a different order than
// it is stored, which matters when the row is a path.
fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Columns the text occupies on a terminal.
pub(super) fn width(value: &str) -> usize {
    value.width()
}

/// Shorten to at most `max` columns, marking a cut with "...".
pub(super) fn fit(value: &str, max: usize) -> String {
    if value.width() <= max {
        return value.to_owned();
    }
    if max <= CUT_MARK.len() {
        return prefix_within(value, max).to_owned();
    }
    format!("{}{CUT_MARK}", prefix_within(value, max - CUT_MARK.len()))
}

/// Break text at spaces into lines of at most `max` columns, so a caller can
/// indent every line; a word longer than a line is cut.
pub(super) fn wrap(value: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if max == 0 {
        return lines;
    }
    let mut line = String::new();
    for word in value.split_whitespace() {
        let word = fit(word, max);
        if !line.is_empty() && line.width() + 1 + word.width() > max {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Wrap exact values (IDs and paths) without dropping characters in long
/// words. Callers sanitize once at the rendering boundary.
pub(super) fn wrap_exact(mut value: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if max == 0 {
        return lines;
    }
    while !value.is_empty() {
        let part = prefix_within(value, max);
        if part.is_empty() {
            break;
        }
        lines.push(part.to_owned());
        value = &value[part.len()..];
    }
    lines
}

/// Keep the end of the text, for input that grows at the end.
pub(super) fn tail_within(value: &str, max: usize) -> &str {
    let mut start = value.len();
    for (index, _) in value.char_indices().rev() {
        if value[index..].width() > max {
            break;
        }
        start = index;
    }
    &value[start..]
}

// Widths are measured on whole prefixes rather than summed per character, so
// sequences that unicode-width treats as one glyph (emoji with a variation
// selector, for example) are measured the way the renderer measures them. A
// zero-width mark stays with the character before it.
pub(super) fn prefix_within(value: &str, max: usize) -> &str {
    let mut end = 0;
    for (index, ch) in value.char_indices() {
        let next = index + ch.len_utf8();
        if value[..next].width() > max {
            break;
        }
        end = next;
    }
    &value[..end]
}

/// Show a path below the user's home directory from `~`.
pub(super) fn home_relative(path: &str, user_home: Option<&Path>) -> String {
    let Some(user_home) = user_home else {
        return path.to_owned();
    };
    match Path::new(path).strip_prefix(user_home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        Err(_) => path.to_owned(),
    }
}

/// A short age such as "12s ago".
pub(super) fn ago(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0..60 => format!("{seconds}s ago"),
        60..3600 => format!("{}m ago", seconds / 60),
        _ => format!("{}h ago", seconds / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::{ago, fit, home_relative, one_line, tail_within, width, wrap};
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn control_and_direction_characters_never_reach_the_screen() {
        assert_eq!(one_line("a\u{1b}[31mb\u{7}c"), "a[31mbc");
        assert_eq!(one_line("line one\nline\ttwo\r"), "line one line two ");
        assert_eq!(one_line("C1\u{9b}2J"), "C12J");
        assert_eq!(one_line("abc\u{202E}fed"), "abcfed");
        assert_eq!(one_line("日本語 ok"), "日本語 ok");
    }

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(fit("~/dev/app", 9), "~/dev/app");
        assert_eq!(fit("", 0), "");
    }

    #[test]
    fn long_text_is_cut_to_the_column_budget_with_a_mark() {
        assert_eq!(fit("~/dev/a-long-project-name", 12), "~/dev/a-l...");
        assert_eq!(width(&fit("~/dev/a-long-project-name", 12)), 12);
        assert_eq!(fit("abcdef", 3), "abc");
        assert_eq!(fit("abcdef", 0), "");
    }

    #[test]
    fn wide_characters_are_never_split_across_the_budget() {
        let cut = fit("日本語のプロジェクト", 9);
        assert_eq!(cut, "日本語...");
        assert!(width(&cut) <= 9);
        let cut = fit("日本語のプロジェクト", 10);
        assert_eq!(cut, "日本語...");
        assert!(width(&cut) <= 10);
    }

    #[test]
    fn combining_marks_stay_with_their_letter() {
        let decomposed = "cafe\u{301} au lait";
        let cut = fit(decomposed, 7);
        assert_eq!(cut, "cafe\u{301}...");
        assert_eq!(width(&cut), 7);
    }

    #[test]
    fn emoji_are_measured_as_the_renderer_measures_them() {
        for text in [
            "ok \u{2764}\u{fe0f} done",
            "fix \u{1f41b} bug",
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} team",
        ] {
            for max in 0..=width(text) {
                assert!(width(&fit(text, max)) <= max, "{text:?} cut to {max}");
            }
        }
    }

    #[test]
    fn wrapping_breaks_at_spaces_within_the_budget() {
        assert_eq!(
            wrap("Start or restart Codex, then send any message.", 20),
            ["Start or restart", "Codex, then send any", "message."]
        );
        assert_eq!(wrap("a  b", 10), ["a b"]);
        assert_eq!(wrap("averyveryverylongword x", 8), ["avery...", "x"]);
        assert!(wrap("anything", 0).is_empty());
        assert!(wrap("", 10).is_empty());
    }

    #[test]
    fn input_tail_keeps_the_end_within_budget() {
        assert_eq!(tail_within("/setup", 10), "/setup");
        assert_eq!(tail_within("abcdefgh", 3), "fgh");
        assert_eq!(tail_within("日本語", 3), "語");
        assert_eq!(tail_within("abc", 0), "");
    }

    #[test]
    fn paths_below_home_start_from_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(home_relative("/home/me", Some(home)), "~");
        assert_eq!(
            home_relative("/home/me/dev/app", Some(home)),
            format!("~{}dev/app", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(
            home_relative("/home/meow/app", Some(home)),
            "/home/meow/app"
        );
        assert_eq!(home_relative("/srv/app", Some(home)), "/srv/app");
        assert_eq!(home_relative("/srv/app", None), "/srv/app");
    }

    #[test]
    fn ages_use_the_largest_whole_unit() {
        assert_eq!(ago(Duration::from_secs(0)), "0s ago");
        assert_eq!(ago(Duration::from_secs(59)), "59s ago");
        assert_eq!(ago(Duration::from_secs(61)), "1m ago");
        assert_eq!(ago(Duration::from_secs(7300)), "2h ago");
    }
}

#[cfg(test)]
mod exact_value_tests {
    use super::{width, wrap_exact};

    #[test]
    fn long_ids_and_paths_wrap_without_losing_characters() {
        for value in [
            "claude:boot:1114701:30037069",
            "/srv/日本語のパス with spaces/long directory name",
            "project-🐛-♥️-source",
        ] {
            for columns in [6, 14, 30] {
                let parts = wrap_exact(value, columns);
                assert_eq!(parts.concat(), value);
                assert!(parts.iter().all(|part| width(part) <= columns));
            }
        }
    }
}
