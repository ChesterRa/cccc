use fs2::FileExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    sync_dir(parent)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

pub fn write_json_committed<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    write_json_committed_with(path, value, write_json)
}

pub(crate) fn write_json_committed_with<T>(
    path: &Path,
    value: &T,
    write: impl FnOnce(&Path, &T) -> io::Result<()>,
) -> io::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    match write(path, value) {
        Ok(()) => Ok(()),
        Err(error) => match read_json::<T>(path) {
            Ok(actual) if actual == *value => Ok(()),
            _ => Err(error),
        },
    }
}

pub fn write_secret_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    write_json(path, value)?;
    protect(path)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

pub fn write_yaml<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_yaml::to_string(value).map_err(io::Error::other)?;
    let text = quote_rfc3339_yaml_scalars(&text);
    atomic_write(path, text.as_bytes())
}

fn quote_rfc3339_yaml_scalars(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let body = line.strip_suffix('\n').unwrap_or(line);
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            let Some((prefix, scalar)) = yaml_scalar_parts(body) else {
                return line.to_owned();
            };
            if chrono::DateTime::parse_from_rfc3339(scalar).is_err() {
                return line.to_owned();
            }
            format!(
                "{prefix}{}{}",
                serde_json::to_string(scalar).unwrap_or_else(|_| "\"\"".into()),
                newline
            )
        })
        .collect()
}

fn yaml_scalar_parts(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    if let Some(scalar) = trimmed.strip_prefix("- ") {
        let prefix_len = line.len() - trimmed.len() + 2;
        return Some((&line[..prefix_len], scalar.trim()));
    }
    let separator = line.find(": ")?;
    let scalar_start = separator + 2;
    Some((&line[..scalar_start], line[scalar_start..].trim()))
}

pub fn write_yaml_committed<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    match write_yaml(path, value) {
        Ok(()) => Ok(()),
        Err(error) => match read_yaml::<T>(path) {
            Ok(actual) if actual == *value => Ok(()),
            _ => Err(error),
        },
    }
}

pub fn write_secret_yaml<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    write_yaml(path, value)?;
    protect(path)
}

pub fn read_yaml<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_yaml::from_reader(File::open(path)?).map_err(io::Error::other)
}

pub fn with_exclusive_lock<T>(
    path: &Path,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock_exclusive()?;
    let result = operation();
    // The descriptor is dropped on return and releases the advisory lock even if an explicit
    // unlock reports an OS-level error. Do not turn a committed operation into an ambiguous
    // failure after its durable write has already completed.
    let _ = FileExt::unlock(&file);
    result
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(unix)]
fn protect(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn protect(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::quote_rfc3339_yaml_scalars;

    #[test]
    fn python_yaml_loaders_keep_rfc3339_values_as_strings() {
        let output = quote_rfc3339_yaml_scalars(
            "created_at: 2026-07-31T01:02:03.456789Z\nitems:\n- 2026-07-31T01:02:03Z\n",
        );
        assert_eq!(
            output,
            "created_at: \"2026-07-31T01:02:03.456789Z\"\nitems:\n- \"2026-07-31T01:02:03Z\"\n"
        );
    }
}
