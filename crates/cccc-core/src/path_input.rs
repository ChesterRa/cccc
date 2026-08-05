use directories::UserDirs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct PreparedDirectory {
    pub path: PathBuf,
    pub created: bool,
}

pub fn expand_user_path(raw: &str) -> io::Result<PathBuf> {
    let trimmed = raw.trim();
    let expanded = if trimmed == "~" {
        home_dir()?
    } else if let Some(relative) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        home_dir()?.join(relative)
    } else {
        PathBuf::from(trimmed)
    };
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(std::env::current_dir()?.join(expanded))
    }
}

pub fn resolve_existing_directory(raw: &str) -> io::Result<PathBuf> {
    let path = expand_user_path(raw)?;
    let resolved = path.canonicalize()?;
    if !resolved.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("not a directory: {}", path.display()),
        ));
    }
    Ok(resolved)
}

pub fn ensure_exact_directory(raw: &str) -> io::Result<PreparedDirectory> {
    let trimmed = raw.trim();
    let target = if trimmed == "~"
        || trimmed.starts_with("~/")
        || trimmed.starts_with("~\\")
        || Path::new(trimmed).is_absolute()
    {
        expand_user_path(trimmed)?
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project path must be absolute or start with ~",
        ));
    };
    if target.exists() {
        return resolve_existing_directory(raw).map(|path| PreparedDirectory {
            path,
            created: false,
        });
    }
    let name = target.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "target directory name is required",
        )
    })?;
    let parent = target.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "target directory parent is required",
        )
    })?;
    let parent = parent.canonicalize()?;
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("not a directory: {}", parent.display()),
        ));
    }
    let exact = parent.join(name);
    std::fs::create_dir(&exact)?;
    match exact.canonicalize() {
        Ok(path) => Ok(PreparedDirectory {
            path,
            created: true,
        }),
        Err(error) => match std::fs::remove_dir(&exact) {
            Ok(()) => Err(error),
            Err(rollback) => Err(io::Error::other(format!(
                "{error}; rollback_failed: could not remove {}: {rollback}",
                exact.display()
            ))),
        },
    }
}

fn home_dir() -> io::Result<PathBuf> {
    UserDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable"))
}

pub fn remove_if_created_empty(directory: &PreparedDirectory) -> io::Result<()> {
    if directory.created {
        std::fs::remove_dir(&directory.path)?;
    }
    Ok(())
}

pub fn parent(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    (parent != path).then(|| parent.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::ensure_exact_directory;

    #[test]
    fn exact_creation_does_not_create_missing_parents() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("missing").join("target");
        assert!(ensure_exact_directory(&target.to_string_lossy()).is_err());
        assert!(!target.exists());
        assert!(!temp.path().join("missing").exists());
    }
}
