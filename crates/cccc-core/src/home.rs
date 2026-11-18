use directories::BaseDirs;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const MARKER: &str = ".cccc-rust-v1";

#[derive(Debug, Error)]
pub enum HomeError {
    #[error("cannot determine the current user's home directory")]
    MissingUserHome,
    #[error("refusing non-empty directory that is not a CCCC home: {0}")]
    UnmarkedDirectory(PathBuf),
    #[error("failed to access CCCC home {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeLayout {
    root: PathBuf,
}

/// A CCCC context variable (CCCC_HOME, CCCC_GROUP_ID or CCCC_ACTOR_ID) as this
/// process received it. Two exact values are reserved as the variable's own
/// unresolved placeholder and count as unset, for an MCP server started outside
/// a CCCC member, where the variable is not defined: Cursor passes
/// `${env:NAME}` verbatim, and Auggie expands the `${NAME:-$NAME}` that CCCC
/// registers to `$NAME`. Any other value, even one containing either text, is
/// kept. Callers keep their own rules for empty values.
pub fn context_var_os(name: &str) -> Option<std::ffi::OsString> {
    without_own_placeholder(name, std::env::var_os(name))
}

fn without_own_placeholder(
    name: &str,
    value: Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    value.filter(|value| {
        *value != std::ffi::OsString::from(format!("${{env:{name}}}"))
            && *value != std::ffi::OsString::from(format!("${name}"))
    })
}

/// [`context_var_os`] for a value that must be Unicode text.
pub fn context_var(name: &str) -> Option<String> {
    context_var_os(name)?.into_string().ok()
}

impl HomeLayout {
    pub fn resolve() -> Result<Self, HomeError> {
        let base = BaseDirs::new().ok_or(HomeError::MissingUserHome)?;
        let configured = context_var_os("CCCC_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".cccc"));
        Self::from_path_with_user_home(configured, base.home_dir())
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self, HomeError> {
        let base = BaseDirs::new().ok_or(HomeError::MissingUserHome)?;
        Self::from_path_with_user_home(path.into(), base.home_dir())
    }

    fn from_path_with_user_home(path: PathBuf, user_home: &Path) -> Result<Self, HomeError> {
        let root = absolute(expand_tilde(path, user_home))?;
        Ok(Self { root })
    }

    pub fn initialize(&self) -> Result<(), HomeError> {
        if self.marker().is_file() && self.daemon_dir().is_dir() && self.groups_dir().is_dir() {
            return Ok(());
        }
        if self.root.exists() && !self.marker().exists() {
            let mut entries = fs::read_dir(&self.root).map_err(|source| self.io(source))?;
            if entries.next().is_some() && !self.is_existing_home() {
                return Err(HomeError::UnmarkedDirectory(self.root.clone()));
            }
        }
        for path in [self.root.clone(), self.daemon_dir(), self.groups_dir()] {
            fs::create_dir_all(&path).map_err(|source| self.io(source))?;
        }
        if !self.marker().exists() {
            fs::write(self.marker(), b"CCCC Rust home v1\n").map_err(|source| self.io(source))?;
        }
        Ok(())
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
    #[must_use]
    pub fn daemon_dir(&self) -> PathBuf {
        self.root.join("daemon")
    }
    #[must_use]
    pub fn groups_dir(&self) -> PathBuf {
        self.root.join("groups")
    }
    #[must_use]
    pub fn registry_path(&self) -> PathBuf {
        self.root.join("registry.json")
    }
    #[must_use]
    pub fn marker(&self) -> PathBuf {
        self.root.join(MARKER)
    }

    fn is_existing_home(&self) -> bool {
        self.root.join(".initialized").is_file()
            || self.registry_path().is_file()
            || self.groups_dir().is_dir()
    }

    fn io(&self, source: std::io::Error) -> HomeError {
        HomeError::Io {
            path: self.root.clone(),
            source,
        }
    }
}

fn expand_tilde(path: PathBuf, user_home: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return user_home.to_path_buf();
    }
    raw.strip_prefix("~/")
        .map_or(path.clone(), |tail| user_home.join(tail))
}

fn absolute(path: PathBuf) -> Result<PathBuf, HomeError> {
    if path.is_absolute() {
        return Ok(path);
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|source| HomeError::Io {
            path: PathBuf::from("."),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopts_existing_python_home_without_changing_existing_data() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join(".cccc");
        fs::create_dir_all(&root).expect("home");
        fs::write(root.join(".initialized"), "python home\n").expect("legacy marker");
        fs::write(root.join("existing-data"), "keep me\n").expect("existing data");

        let home = HomeLayout::from_path_with_user_home(root.clone(), temp.path()).expect("layout");
        home.initialize().expect("initialize");

        assert_eq!(
            fs::read_to_string(root.join("existing-data")).expect("existing data"),
            "keep me\n"
        );
        assert!(root.join(MARKER).is_file());
    }

    #[test]
    fn marks_new_home_and_reopens_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("rust-home");
        let home = HomeLayout::from_path(&root).expect("layout");
        home.initialize().expect("initialize");
        assert!(root.join(MARKER).is_file());
        home.initialize().expect("reopen marked home");
    }

    #[test]
    fn only_a_context_variables_own_unresolved_placeholder_counts_as_unset() {
        use std::ffi::OsString;
        let value = |text: &str| Some(OsString::from(text));
        for placeholder in ["${env:CCCC_HOME}", "$CCCC_HOME"] {
            assert_eq!(
                without_own_placeholder("CCCC_HOME", value(placeholder)),
                None
            );
        }
        assert_eq!(without_own_placeholder("CCCC_HOME", None), None);
        for kept in [
            "/srv/cccc",
            "",
            "${env:CCCC_GROUP_ID}",
            "${env:CCCC_HOME}/x",
            "$CCCC_GROUP_ID",
            "./$CCCC_HOME",
            "/tmp/$CCCC_HOME",
            "$CCCC_HOME ",
            "${CCCC_HOME:-$CCCC_HOME}",
        ] {
            assert_eq!(
                without_own_placeholder("CCCC_HOME", value(kept)),
                value(kept)
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            for bytes in [&b"/srv/\xffhome"[..], &b"$CCCC_HOME\xff"[..]] {
                let path = OsString::from_vec(bytes.to_vec());
                assert_eq!(
                    without_own_placeholder("CCCC_HOME", Some(path.clone())),
                    Some(path)
                );
            }
        }
    }

    #[test]
    fn refuses_non_empty_unmarked_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("existing"), "data").expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("layout");
        assert!(matches!(
            home.initialize(),
            Err(HomeError::UnmarkedDirectory(_))
        ));
    }
}
