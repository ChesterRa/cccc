//! Pinned official ACP distribution. Installed locally, never bundled with CCCC.
use cccc_core::HomeLayout;
use fs2::FileExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const VERSION: &str = "1.3.0";
pub async fn login(home: &HomeLayout) -> io::Result<()> {
    crate::ops::codex_voice_analyst::login_antigravity(home).await
}
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

pub struct Installation {
    pub command: Vec<String>,
    pub provider_home: PathBuf,
    pub environment: BTreeMap<String, String>,
}

fn package() -> io::Result<(&'static str, &'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok((
            "linux",
            "linux-x86_64",
            "9fb60956af0a9d76220a4db91ca9ac88e2a2372ad68f985ab5fceace6b825b96",
        )),
        ("linux", "aarch64") => Ok((
            "linux",
            "linux-arm64",
            "500b0bc0fb858e88f4df404d4cedf80bf9298c178291e39e383d6c50b111cbdf",
        )),
        ("macos", "x86_64") => Ok((
            "macos",
            "darwin-x86_64",
            "bb23956b89984bf5d354af2c3725e6c57f0cc1b7228e77a0e91c9c2bc1d47646",
        )),
        ("macos", "aarch64") => Ok((
            "macos",
            "darwin-arm64",
            "7cd97045f7b4fe81175a107cdf16f9c51484e3c78a5162cae415338bb6aa5b88",
        )),
        ("windows", "x86_64") => Ok((
            "windows",
            "windows-x86_64",
            "65215e0688681fa3116e048a9eab27ef53af1bbd6f3da3f1c52bd4911d8b17f9",
        )),
        ("windows", "aarch64") => Ok((
            "windows",
            "windows-arm64",
            "4a0f469720e9beb9438a979f543fdbfad5022ebe0992c052c590bd78b3144ca3",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Official Antigravity ACP has no distribution for this platform",
        )),
    }
}

fn names() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("agy_acp_server.exe", "localharness_external.exe")
    } else {
        ("agy_acp_server.par", "localharness_external")
    }
}

pub fn provider_home(home: &HomeLayout) -> PathBuf {
    home.root().join("state/antigravity-acp/home")
}

/// Read only. Discovery never installs or authenticates a runtime.
pub fn installed(home: &HomeLayout) -> bool {
    let Ok((_, platform, digest)) = package() else {
        return false;
    };
    valid_install(
        &home
            .root()
            .join("runtimes/antigravity-acp")
            .join(VERSION)
            .join(platform),
        digest,
    )
}

#[cfg(all(test, unix))]
pub(crate) fn install_fixture(home: &HomeLayout, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let (_, platform, digest) = package().expect("fixture platform");
    let root = home
        .root()
        .join("runtimes/antigravity-acp")
        .join(VERSION)
        .join(platform);
    fs::create_dir_all(&root).expect("fixture directory");
    let (server, harness) = names();
    fs::write(root.join(server), script).expect("fixture script");
    fs::set_permissions(root.join(server), fs::Permissions::from_mode(0o700))
        .expect("fixture mode");
    fs::write(root.join(harness), b"fixture").expect("fixture harness");
    cccc_core::fs::write_json(
        &root.join("installation.json"),
        &json!({"version":VERSION,"archive_sha256":digest}),
    )
    .expect("fixture manifest");
    root
}

fn valid_install(root: &Path, digest: &str) -> bool {
    let (server, harness) = names();
    root.join(server).is_file()
        && root.join(harness).is_file()
        && cccc_core::fs::read_json::<serde_json::Value>(&root.join("installation.json"))
            .is_ok_and(|value| value == json!({"version":VERSION,"archive_sha256":digest}))
}

pub fn ensure(home: &HomeLayout) -> io::Result<Installation> {
    let (folder, platform, digest) = package()?;
    let parent = home.root().join("runtimes/antigravity-acp").join(VERSION);
    fs::create_dir_all(&parent)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join("install.lock"))?;
    lock.lock_exclusive()?;
    let root = parent.join(platform);
    if !valid_install(&root, digest) {
        if root.exists() {
            return Err(io::Error::other(
                "Antigravity ACP installation is incomplete; remove only its managed distribution directory and run setup again",
            ));
        }
        let staging = tempfile::tempdir_in(&parent)?;
        let mut archive = tempfile::tempfile_in(&parent)?;
        let url = format!(
            "https://dl.google.com/agy-extensions/releases/{folder}/agy-acp-server-{VERSION}-{platform}.zip"
        );
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(io::Error::other)?;
        let mut response = client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(io::Error::other)?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let size = response.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            bytes += size as u64;
            if bytes > MAX_ARCHIVE_BYTES {
                return Err(io::Error::other(
                    "Antigravity ACP archive exceeds the download limit",
                ));
            }
            hash.update(&buffer[..size]);
            archive.write_all(&buffer[..size])?;
        }
        if format!("{:x}", hash.finalize()) != digest {
            return Err(io::Error::other(
                "Official Antigravity ACP archive checksum does not match the pinned release",
            ));
        }
        extract(archive, staging.path(), names())?;
        cccc_core::fs::write_json(
            &staging.path().join("installation.json"),
            &json!({"version":VERSION,"archive_sha256":digest}),
        )?;
        fs::rename(staging.path(), &root)?;
    }
    let provider_home = provider_home(home);
    fs::create_dir_all(&provider_home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&provider_home, fs::Permissions::from_mode(0o700))?;
    }
    let (server, harness) = names();
    let mut command = vec![root.join(server).to_string_lossy().into_owned()];
    if cfg!(target_os = "linux") {
        command.push("--uid=".into());
    }
    let environment = BTreeMap::from([
        ("HOME".into(), provider_home.to_string_lossy().into_owned()),
        (
            "USERPROFILE".into(),
            provider_home.to_string_lossy().into_owned(),
        ),
        (
            "GEMINI_HOME".into(),
            provider_home.join(".gemini").to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            provider_home.join(".config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            provider_home
                .join(".local/share")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "XDG_CACHE_HOME".into(),
            provider_home.join(".cache").to_string_lossy().into_owned(),
        ),
        (
            "ANTIGRAVITY_HARNESS_PATH".into(),
            root.join(harness).to_string_lossy().into_owned(),
        ),
    ]);
    Ok(Installation {
        command,
        provider_home,
        environment,
    })
}

fn extract(file: fs::File, root: &Path, names: (&str, &str)) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(file).map_err(io::Error::other)?;
    for name in [names.0, names.1] {
        let mut entry = archive.by_name(name).map_err(io::Error::other)?;
        if !entry.is_file() || entry.size() > 1024 * 1024 * 1024 {
            return Err(io::Error::other("Invalid official ACP archive entry"));
        }
        let path = root.join(name);
        let mut output = fs::File::create(&path)?;
        io::copy(&mut entry, &mut output)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;
    #[test]
    fn discovery_does_not_create_provider_state() {
        let temp = tempfile::tempdir().expect("fixture operation");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("fixture operation");
        assert!(!installed(&home));
        assert!(!home.root().exists());
    }
    #[test]
    fn extraction_selects_only_pinned_executables() {
        let mut file = tempfile::tempfile().expect("fixture operation");
        {
            let mut zip = zip::ZipWriter::new(&mut file);
            for name in ["server", "harness", "../outside"] {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("fixture operation");
                zip.write_all(b"fixture").expect("fixture operation");
            }
            zip.finish().expect("fixture operation");
        }
        file.rewind().expect("fixture operation");
        let temp = tempfile::tempdir().expect("fixture operation");
        extract(file, temp.path(), ("server", "harness")).expect("fixture operation");
        assert_eq!(
            fs::read(temp.path().join("server")).expect("fixture operation"),
            b"fixture"
        );
        assert_eq!(
            fs::read_dir(temp.path())
                .expect("fixture operation")
                .count(),
            2
        );
    }
}
