use std::path::Path;

/// The spelling of a canonical path to show to a person or model, or to give a
/// launcher that passes a working directory to Windows unchanged.
///
/// On Windows `fs::canonicalize` returns `\\?\C:\...`. `std::process::Command`
/// removes or converts that prefix in a working directory when it is safe to;
/// portable-pty passes it on unchanged, and cmd.exe started there falls back to
/// the Windows directory. The prefix is dropped only when the drive path names
/// the same location; UNC paths, reserved names and paths over the legacy length
/// keep it, and cmd.exe may still fall back for UNC paths in either spelling.
/// Compare canonical paths, not this form: the two spellings of one directory
/// differ under `starts_with`.
pub fn compatible_path(path: &Path) -> &Path {
    dunce::simplified(path)
}

#[cfg(test)]
mod tests {
    use super::compatible_path;
    use std::path::Path;

    #[cfg(not(windows))]
    #[test]
    fn other_platforms_keep_the_path() {
        for path in ["/tmp/project", r"\\?\C:\project"] {
            assert_eq!(compatible_path(Path::new(path)), Path::new(path));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_drops_the_verbatim_prefix_only_where_the_drive_path_is_equivalent() {
        let at_limit = format!(r"\\?\C:\{}", "a".repeat(253));
        assert_eq!(at_limit.len(), 260);
        for (path, expected) in [
            (r"\\?\C:\Users\demo", r"C:\Users\demo"),
            (r"C:\Users\demo", r"C:\Users\demo"),
            (at_limit.as_str(), &at_limit[4..]),
        ] {
            assert_eq!(compatible_path(Path::new(path)), Path::new(expected));
        }
        let over_limit = format!(r"\\?\C:\{}", "a".repeat(254));
        for path in [
            r"\\?\UNC\server\share\demo",
            r"\\?\C:\work\aux",
            r"\\?\C:\work\trailing.",
            over_limit.as_str(),
        ] {
            assert_eq!(compatible_path(Path::new(path)), Path::new(path));
        }
    }
}
