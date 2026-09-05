use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(crate) const MAGIC: &[u8; 8] = b"CCCCPTY1";
pub(crate) const HEADER_BYTES: u64 = 16;

pub(crate) fn publish_latest(parent: &Path, path: &Path) -> std::io::Result<()> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| std::io::Error::other("invalid terminal transcript filename"))?;
    let temp = parent.join("latest.tmp");
    fs::write(&temp, name.as_bytes())?;
    replace_file(&temp, &parent.join("latest"))
}

pub(crate) fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        match fs::remove_file(target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(source, target)
    }
    #[cfg(not(windows))]
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_file(target)?;
            fs::rename(source, target)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn latest_path(parent: &Path) -> std::io::Result<PathBuf> {
    let name = fs::read_to_string(parent.join("latest"))?;
    let name = name.trim();
    if name.is_empty() || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(std::io::Error::other(
            "invalid latest terminal transcript pointer",
        ));
    }
    Ok(parent.join(name))
}

pub(crate) fn transcript_bounds(path: &Path) -> std::io::Result<(u64, u64)> {
    let mut file = File::open(path)?;
    let mut header = [0_u8; HEADER_BYTES as usize];
    file.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(std::io::Error::other("invalid terminal transcript header"));
    }
    let start = u64::from_le_bytes(header[8..16].try_into().unwrap_or_default());
    let data_len = file.metadata()?.len().saturating_sub(HEADER_BYTES);
    Ok((start, start.saturating_add(data_len)))
}

pub(crate) fn latest_end(parent: &Path) -> std::io::Result<u64> {
    match latest_path(parent) {
        Ok(path) => transcript_bounds(&path).map(|(_, end)| end),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

pub(crate) fn prune_sessions(parent: &Path, max_bytes: u64, current: &Path) -> std::io::Result<()> {
    let mut files = fs::read_dir(parent)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pty") && path != current)
        .filter_map(|path| {
            let metadata = path.metadata().ok()?;
            let (start, _) = transcript_bounds(&path).ok()?;
            Some((
                start,
                metadata.modified().ok(),
                metadata.len().saturating_sub(HEADER_BYTES),
                path,
            ))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|item| (item.0, item.1));
    let current_bytes = current
        .metadata()
        .map(|meta| meta.len().saturating_sub(HEADER_BYTES))
        .unwrap_or(0);
    let mut total = current_bytes.saturating_add(files.iter().map(|item| item.2).sum::<u64>());
    for (_, _, bytes, path) in files {
        if total <= max_bytes.max(current_bytes) {
            break;
        }
        fs::remove_file(path)?;
        total = total.saturating_sub(bytes);
    }
    Ok(())
}

pub(crate) fn remove_other_sessions(parent: &Path, current: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(parent)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "pty") && path != current {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(crate) fn secure_create(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::{latest_path, publish_latest};

    #[test]
    fn publish_latest_replaces_an_existing_pointer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = temp.path().join("first.pty");
        let second = temp.path().join("second.pty");
        publish_latest(temp.path(), &first).expect("publish first");
        publish_latest(temp.path(), &second).expect("replace latest");
        assert_eq!(latest_path(temp.path()).expect("latest"), second);
    }
}
