use std::path::{Path, PathBuf};

#[cfg(windows)]
pub(super) fn filesystem_roots() -> Vec<PathBuf> {
    windows_drive_candidates()
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .collect()
}

#[cfg(not(windows))]
pub(super) fn filesystem_roots() -> Vec<PathBuf> {
    Vec::new()
}

pub(super) fn drive_label(path: &Path) -> String {
    display_path(path).trim_end_matches(['\\', '/']).to_owned()
}

pub(super) fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        strip_windows_verbatim_prefix(&value)
    }
    #[cfg(not(windows))]
    {
        value.into_owned()
    }
}

#[cfg(any(windows, test))]
fn windows_drive_candidates() -> Vec<String> {
    ('A'..='Z').map(|letter| format!(r"{letter}:\")).collect()
}

#[cfg(any(windows, test))]
fn strip_windows_verbatim_prefix(value: &str) -> String {
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
}

#[cfg(test)]
mod tests {
    use super::{strip_windows_verbatim_prefix, windows_drive_candidates};

    #[test]
    fn windows_drive_candidates_cover_every_drive_letter() {
        let candidates = windows_drive_candidates();
        assert_eq!(candidates.len(), 26);
        assert_eq!(candidates.first().map(String::as_str), Some(r"A:\"));
        assert_eq!(candidates.last().map(String::as_str), Some(r"Z:\"));
    }

    #[test]
    fn windows_verbatim_prefixes_are_removed_for_display() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\C:\Users\demo"),
            r"C:\Users\demo"
        );
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\UNC\server\share\demo"),
            r"\\server\share\demo"
        );
    }
}
