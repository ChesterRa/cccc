/// Split a message without dropping or rewriting any Unicode scalar values.
///
/// Platform limits are expressed in characters rather than UTF-8 bytes.  An
/// optional line limit is also enforced for APIs (DingTalk/WeCom) that reject
/// otherwise short messages containing too many lines.
pub(super) fn split_message(text: &str, max_chars: usize, max_lines: Option<usize>) -> Vec<String> {
    if text.is_empty() || max_chars == 0 {
        return Vec::new();
    }
    let max_lines = max_lines.filter(|limit| *limit > 0);
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    let mut lines = 1;

    for (index, character) in text.char_indices() {
        let exceeds_chars = chars >= max_chars;
        let exceeds_lines = character == '\n' && max_lines.is_some_and(|limit| lines >= limit);
        if index > start && (exceeds_chars || exceeds_lines) {
            chunks.push(text[start..index].to_owned());
            start = index;
            chars = 0;
            lines = 1;
        }
        chars += 1;
        if character == '\n' {
            lines += 1;
        }
    }
    if start < text.len() {
        chunks.push(text[start..].to_owned());
    }
    chunks
}

pub(super) fn fits_message(text: &str, max_chars: usize, max_lines: Option<usize>) -> bool {
    text.chars().count() <= max_chars
        && max_lines.is_none_or(|limit| text.split('\n').count() <= limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_is_unicode_safe_and_lossless() {
        let text = "你好🙂abcdef";
        let chunks = split_message(text, 3, None);
        assert_eq!(chunks, ["你好🙂", "abc", "def"]);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn split_enforces_line_limit_without_losing_newlines() {
        let text = "one\ntwo\nthree\nfour";
        let chunks = split_message(text, 100, Some(2));
        assert!(chunks.iter().all(|chunk| chunk.split('\n').count() <= 2));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn trailing_newline_counts_as_an_additional_logical_line() {
        let text = "one\ntwo\n";
        assert!(!fits_message(text, 100, Some(2)));
        let chunks = split_message(text, 100, Some(2));
        assert!(chunks.iter().all(|chunk| chunk.split('\n').count() <= 2));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn fit_check_counts_characters_instead_of_utf8_bytes() {
        assert!(fits_message("你好🙂", 3, None));
        assert!(!fits_message("你好🙂", 2, None));
    }
}
