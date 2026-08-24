use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use tar::Archive;

use crate::HomeLayout;
use crate::fs;

pub const PINNED_VERSION: &str = "2026.8.2";
pub const MAX_DOWNLOAD_BYTES: usize = 80 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Artifact {
    pub name: &'static str,
    pub artifact_sha256: &'static str,
    pub binary_sha256: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inspection {
    pub supported: bool,
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub pinned_version: &'static str,
    pub matches_pin: bool,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct InstallMetadata {
    #[serde(default)]
    version: String,
    #[serde(default)]
    artifact: String,
    #[serde(default)]
    artifact_sha256: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    installed_at: String,
}

pub fn artifact_for(system: &str, machine: &str) -> Option<Artifact> {
    match (system, machine) {
        ("linux", "x86_64" | "amd64") => Some(Artifact {
            name: "cloudflared-linux-amd64",
            artifact_sha256: "fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2",
            binary_sha256: "fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2",
        }),
        ("linux", "aarch64" | "arm64") => Some(Artifact {
            name: "cloudflared-linux-arm64",
            artifact_sha256: "7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790",
            binary_sha256: "7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790",
        }),
        ("darwin", "x86_64" | "amd64") => Some(Artifact {
            name: "cloudflared-darwin-amd64.tgz",
            artifact_sha256: "f1727723c586500e2092368ae21871b3df7ddfd2cb097f22d81bee4a9c458bb4",
            binary_sha256: "b0f770e1e0b281399a57219b840fd8eef1cc25387a404124248157ea2073727a",
        }),
        ("darwin", "aarch64" | "arm64") => Some(Artifact {
            name: "cloudflared-darwin-arm64.tgz",
            artifact_sha256: "9042c2c5d8b2de78e60f313d5fb31b6c5c1cebde787a3caf1f2c9588084ac442",
            binary_sha256: "b61054d3d6326ea558cb49826eebf5676e0d0a36d51b546975096ca3e0e3c89d",
        }),
        _ => None,
    }
}

pub fn binary_path(home: &HomeLayout) -> PathBuf {
    home.root()
        .join("libexec")
        .join("cloudflared")
        .join("cloudflared")
}

pub fn install_dir(home: &HomeLayout) -> PathBuf {
    home.root().join("libexec").join("cloudflared")
}

pub fn metadata_path(home: &HomeLayout) -> PathBuf {
    install_dir(home).join("install.json")
}

pub fn current_platform() -> (&'static str, &'static str) {
    let system = match std::env::consts::OS {
        "macos" => "darwin",
        value => value,
    };
    (system, std::env::consts::ARCH)
}

pub fn download_url(artifact: &Artifact) -> String {
    let default =
        format!("https://github.com/cloudflare/cloudflared/releases/download/{PINNED_VERSION}");
    let base = std::env::var("CCCC_CLOUDFLARED_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or(default);
    format!("{base}/{}", artifact.name)
}

pub fn sha256_bytes(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

pub fn verify_sha256(data: &[u8], expected: &str) -> Result<(), String> {
    let actual = sha256_bytes(data);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "cloudflared sha256 mismatch (got {actual}, expected {expected})"
        ))
    }
}

pub fn install_bare_binary(home: &HomeLayout, data: &[u8], expected: &str) -> io::Result<PathBuf> {
    verify_sha256(data, expected).map_err(io::Error::other)?;
    let dest = binary_path(home);
    fs::atomic_write(&dest, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dest)
}

pub fn installed_sha256(path: &Path) -> io::Result<String> {
    let data = std::fs::read(path)?;
    Ok(sha256_bytes(&data))
}

pub fn inspect(home: &HomeLayout) -> io::Result<Inspection> {
    let path = binary_path(home);
    let installed = path.is_file();
    let sha256 = installed.then(|| installed_sha256(&path)).transpose()?;
    let metadata = fs::read_json::<InstallMetadata>(&metadata_path(home)).unwrap_or_default();
    let expected = artifact_for(current_platform().0, current_platform().1);
    let matches_pin = installed
        && metadata.version == PINNED_VERSION
        && expected.is_some_and(|artifact| sha256.as_deref() == Some(artifact.binary_sha256));
    Ok(Inspection {
        supported: expected.is_some(),
        installed,
        path: installed.then_some(path),
        version: (!metadata.version.is_empty()).then_some(metadata.version),
        sha256,
        pinned_version: PINNED_VERSION,
        matches_pin,
    })
}

pub fn extract_binary(artifact: &Artifact, payload: &[u8]) -> io::Result<Vec<u8>> {
    if !artifact.name.ends_with(".tgz") {
        return Ok(payload.to_vec());
    }
    let mut archive = Archive::new(GzDecoder::new(Cursor::new(payload)));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let is_binary = entry.header().entry_type().is_file()
            && entry
                .path()?
                .file_name()
                .is_some_and(|name| name == "cloudflared");
        if !is_binary {
            continue;
        }
        let mut binary = Vec::new();
        entry
            .by_ref()
            .take((MAX_DOWNLOAD_BYTES + 1) as u64)
            .read_to_end(&mut binary)?;
        if binary.len() > MAX_DOWNLOAD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cloudflared binary exceeded size limit",
            ));
        }
        return Ok(binary);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "cloudflared archive did not contain a cloudflared binary",
    ))
}

pub fn install_from_bytes(
    home: &HomeLayout,
    payload: &[u8],
    upgrade: bool,
) -> io::Result<Inspection> {
    let (system, machine) = current_platform();
    let artifact = artifact_for(system, machine).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("cloudflared is not provided for {system}/{machine} in this release"),
        )
    })?;
    verify_sha256(payload, artifact.artifact_sha256).map_err(io::Error::other)?;
    let current = inspect(home)?;
    if current.installed && !current.matches_pin && !upgrade {
        return Err(io::Error::other(
            "installed cloudflared is not the pinned release; run `cccc reach install` to upgrade",
        ));
    }
    let binary = extract_binary(&artifact, payload)?;
    verify_sha256(&binary, artifact.binary_sha256).map_err(io::Error::other)?;
    install_bare_binary(home, &binary, artifact.binary_sha256)?;
    fs::write_json(
        &metadata_path(home),
        &InstallMetadata {
            version: PINNED_VERSION.into(),
            artifact: artifact.name.into(),
            artifact_sha256: artifact.artifact_sha256.into(),
            sha256: artifact.binary_sha256.into(),
            installed_at: cccc_contracts::utc_now(),
        },
    )?;
    inspect(home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    #[test]
    fn rejects_bad_hash() {
        let error = verify_sha256(b"nope", "abc").expect_err("hash");
        assert!(error.contains("sha256 mismatch"));
    }

    #[test]
    fn installs_only_after_hash_matches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("init");
        let payload = b"fixture";
        let digest = sha256_bytes(payload);
        let dest = install_bare_binary(&home, payload, &digest).expect("install");
        assert_eq!(std::fs::read(dest).expect("read"), payload);
        assert!(install_bare_binary(&home, payload, "deadbeef").is_err());
    }

    #[test]
    fn pins_linux_and_darwin_only() {
        let linux = artifact_for("linux", "x86_64").expect("linux");
        assert_eq!(linux.artifact_sha256, linux.binary_sha256);
        let darwin = artifact_for("darwin", "arm64").expect("darwin");
        assert_ne!(darwin.artifact_sha256, darwin.binary_sha256);
        assert_eq!(artifact_for("darwin", "aarch64"), Some(darwin));
        assert!(artifact_for("windows", "x86_64").is_none());
    }

    #[test]
    fn extracts_the_binary_from_a_darwin_archive() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut archive = Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(7);
            header.set_mode(0o755);
            header.set_cksum();
            archive
                .append_data(&mut header, "nested/cloudflared", &b"fixture"[..])
                .expect("append");
            archive.finish().expect("finish");
        }
        let payload = encoder.finish().expect("gzip");
        let artifact = artifact_for("darwin", "arm64").expect("artifact");
        assert_eq!(
            extract_binary(&artifact, &payload).expect("extract"),
            b"fixture"
        );
    }

    #[test]
    fn current_macos_name_maps_to_the_release_platform() {
        if std::env::consts::OS == "macos" {
            assert_eq!(current_platform().0, "darwin");
        }
    }
}
