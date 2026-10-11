//! Bounded provider-error text, sanitized before ACP replies or events leave their owner.

use regex::Regex;
use std::sync::LazyLock;

pub(super) const LIMIT: usize = 1600;

static CREDENTIALS: LazyLock<[Regex; 5]> = LazyLock::new(|| {
    [
        r#"(?i)\b(?:api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|token|password|secret|authorization)\b["']?\s*[:=]\s*(?:Bearer[ \t]+)?(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;]+)"#,
        r"(?i)\bBearer[ \t]+[A-Za-z0-9._~+/=-]+",
        r"\b(?:sk-[A-Za-z0-9_-]{8,}|gh[pousr]_[A-Za-z0-9_]{8,}|github_pat_[A-Za-z0-9_]{8,}|AIza[A-Za-z0-9_-]{20,}|xai-[A-Za-z0-9_-]{8,}|gsk_[A-Za-z0-9_-]{8,})\b",
        r"\beyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
        r"(?i)[?&](?:code|state|token|access_token|refresh_token|api_key|secret|sig|signature)=[^&\s]+",
    ]
    .map(|pattern| Regex::new(pattern).expect("valid credential diagnostic pattern"))
});

pub(super) fn sanitize(text: &str, private_values: &[String]) -> String {
    // Match against the original text: truncation or one replacement must not split
    // another secret (including multiline private values and short values inside JWTs).
    let mut spans = Vec::new();
    for pattern in CREDENTIALS.iter() {
        spans.extend(
            pattern
                .find_iter(text)
                .map(|found| (found.start(), found.end())),
        );
    }
    for value in private_values.iter().filter(|value| !value.is_empty()) {
        spans.extend(
            text.match_indices(value)
                .map(|(start, _)| (start, start + value.len())),
        );
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        if let Some(previous) = merged.last_mut()
            && start <= previous.1
        {
            previous.1 = previous.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    let mut safe = String::new();
    let mut cursor = 0;
    for (start, end) in merged {
        safe.push_str(&text[cursor..start]);
        safe.push_str("[redacted]");
        cursor = end;
    }
    safe.push_str(&text[cursor..]);
    // ACP's exception details are useful prose, not authority to expose a traceback.
    safe.lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn diagnostic_redacts_credential_shapes_and_drops_traceback_lines() {
        let text = sanitize(
            "Refresh denied; Bearer opaque-value; api_key=opaque-key; eyJhbGciOiJub25lIn0.eyJleHAiOjF9.c2ln; https://auth.example/callback?code=private-code&state=private-state\nTraceback: DO_NOT_PUBLISH_TRACE",
            &["1".into()],
        );
        assert!(text.starts_with("Refresh denied"));
        for private in [
            "opaque-value",
            "opaque-key",
            "eyJhbGci",
            "private-code",
            "private-state",
            "DO_NOT_PUBLISH_TRACE",
        ] {
            assert!(!text.contains(private));
        }
    }

    #[test]
    fn diagnostic_redacts_complete_multiline_private_values_before_selecting_a_line() {
        let private = "opaque-private-first-line\nBearer private-second-line";
        let safe = sanitize(
            &format!("Refresh denied: {private}\nTraceback: hidden"),
            &[private.into()],
        );
        assert_eq!(safe, "Refresh denied: [redacted]");
    }
}
