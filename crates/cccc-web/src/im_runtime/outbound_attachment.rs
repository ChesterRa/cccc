use std::path::Path;

/// Accept only a single, portable file-name component for outbound uploads.
pub(super) fn safe_filename(value: &str) -> Option<&str> {
    let value = value.trim();
    let path = Path::new(value);
    (!value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\'])
        && !value.chars().any(char::is_control)
        && path.file_name().and_then(|name| name.to_str()) == Some(value))
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_portable_single_components() {
        assert_eq!(safe_filename(" report.md "), Some("report.md"));
        for value in [
            "",
            ".",
            "..",
            "../report.md",
            "folder/report.md",
            "folder\\report.md",
            "bad\nname",
        ] {
            assert_eq!(safe_filename(value), None, "{value:?}");
        }
    }
}
