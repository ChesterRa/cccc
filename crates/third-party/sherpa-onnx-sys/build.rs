use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{collections::HashSet, ffi::OsString};

use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;

const RELEASE_BASE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
const UPSTREAM_VERSION: &str = "1.13.4";
const SHERPA_ONNX_STATIC_LIBS: &[&str] = &[
    "sherpa-onnx-c-api",
    "sherpa-onnx-core",
    "kaldi-decoder-core",
    "sherpa-onnx-kaldifst-core",
    "sherpa-onnx-fstfar",
    "sherpa-onnx-fst",
    "kaldi-native-fbank-core",
    "kissfft-float",
    "piper_phonemize",
    "espeak-ng",
    "ucd",
    "onnxruntime",
    "ssentencepiece_core",
];

type DynError = Box<dyn Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkMode {
    Static,
    Shared,
}

fn main() {
    if let Err(err) = try_main() {
        panic!("{err}");
    }
}

fn try_main() -> Result<(), DynError> {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_ARCHIVE_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    if env::var_os("DOCS_RS").is_some() {
        // docs.rs sets DOCS_RS=1; skip downloading/linking native libraries
        // so that `cargo doc` can succeed without the real C artifacts.
        return Ok(());
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH")?;
    let link_mode = resolve_link_mode()?;
    let lib_dir = resolve_lib_dir(link_mode, &target_os, &target_arch)?;

    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    if link_mode == LinkMode::Shared && matches!(target_os.as_str(), "linux" | "macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
        emit_relative_rpath(&target_os);
        copy_unix_runtime_libs(&lib_dir, &target_os)?;
    }

    if link_mode == LinkMode::Shared && target_os == "windows" {
        copy_windows_runtime_dlls(&lib_dir)?;
    }

    match link_mode {
        LinkMode::Static => emit_static_link_directives(&target_os),
        LinkMode::Shared => emit_shared_link_directives(),
    }

    Ok(())
}

fn resolve_link_mode() -> Result<LinkMode, DynError> {
    let static_enabled = env::var_os("CARGO_FEATURE_STATIC").is_some();
    let shared_enabled = env::var_os("CARGO_FEATURE_SHARED").is_some();

    if static_enabled && shared_enabled {
        return Err("Features `static` and `shared` cannot be enabled at the same time".into());
    }

    if shared_enabled {
        Ok(LinkMode::Shared)
    } else {
        Ok(LinkMode::Static)
    }
}

fn resolve_lib_dir(
    link_mode: LinkMode,
    target_os: &str,
    target_arch: &str,
) -> Result<PathBuf, DynError> {
    if let Some(path) = env::var_os("SHERPA_ONNX_LIB_DIR") {
        let path = PathBuf::from(path);
        if !path.is_dir() {
            return Err(format!(
                "SHERPA_ONNX_LIB_DIR does not exist or is not a directory: {}",
                path.display()
            )
            .into());
        }
        return Ok(path);
    }

    download_prebuilt_libs(link_mode, target_os, target_arch)
}

fn download_prebuilt_libs(
    link_mode: LinkMode,
    target_os: &str,
    target_arch: &str,
) -> Result<PathBuf, DynError> {
    let archive_name = archive_name(link_mode, target_os, target_arch)?;
    let expected_sha256 = archive_sha256(&archive_name)?;
    let archive_stem = archive_name.trim_end_matches(".tar.bz2");

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let cache_root = target_dir_from_out_dir(&out_dir)?.join("sherpa-onnx-prebuilt");
    let extracted_dir = cache_root.join(archive_stem);
    let lib_dir = extracted_dir.join("lib");
    let verified_marker = extracted_dir.join(".archive-sha256");

    if lib_dir.is_dir()
        && fs::read_to_string(&verified_marker).is_ok_and(|value| value.trim() == expected_sha256)
    {
        return Ok(lib_dir);
    }

    if extracted_dir.exists() {
        fs::remove_dir_all(&extracted_dir)?;
    }

    fs::create_dir_all(&cache_root)?;

    let archive_path = cache_root.join(&archive_name);
    if !archive_path.is_file() {
        if let Some(local_archive_dir) = env::var_os("SHERPA_ONNX_ARCHIVE_DIR") {
            let local_archive_path = PathBuf::from(local_archive_dir).join(&archive_name);
            if !local_archive_path.is_file() {
                return Err(format!(
                    "SHERPA_ONNX_ARCHIVE_DIR does not contain expected archive: {}",
                    local_archive_path.display()
                )
                .into());
            }

            copy_file_atomically(&local_archive_path, &archive_path)?;
        } else {
            let url = format!("{RELEASE_BASE_URL}/v{UPSTREAM_VERSION}/{archive_name}");
            eprintln!("Downloading sherpa-onnx libs from {url}");

            let curl_failure = match download_with_curl(&url, &archive_path) {
                Ok(true) => None,
                Ok(false) => Some("curl executable is unavailable".to_owned()),
                Err(error) => Some(error.to_string()),
            };
            if let Some(curl_failure) = curl_failure {
                eprintln!(
                    "curl download did not complete ({curl_failure}); falling back to the built-in HTTP client"
                );
                download_with_ureq(&url, &archive_path).map_err(|fallback_error| {
                    format!(
                        "Failed to download sherpa-onnx archive from {url}; curl: {curl_failure}; built-in HTTP client: {fallback_error}"
                    )
                })?;
            }
        }
    }

    if let Err(error) = verify_archive_sha256(&archive_path, expected_sha256) {
        let _ = fs::remove_file(&archive_path);
        return Err(error);
    }

    let unpack_result: Result<(), DynError> = (|| {
        let tar_file = File::open(&archive_path)?;
        let decoder = BzDecoder::new(tar_file);
        let mut archive = Archive::new(decoder);
        archive.unpack(&cache_root)?;
        Ok(())
    })();
    if let Err(err) = unpack_result {
        let _ = fs::remove_file(&archive_path);
        let _ = fs::remove_dir_all(&extracted_dir);
        return Err(format!(
            "Failed to unpack cached archive {}: {err}",
            archive_path.display()
        )
        .into());
    }

    if !lib_dir.is_dir() {
        return Err(format!(
            "Downloaded archive did not contain a lib directory: {}",
            lib_dir.display()
        )
        .into());
    }
    fs::write(&verified_marker, format!("{expected_sha256}\n"))?;

    eprintln!("Downloaded sherpa-onnx libs to {}", extracted_dir.display());

    Ok(lib_dir)
}

fn archive_sha256(archive_name: &str) -> Result<&'static str, DynError> {
    let sha256 = match archive_name {
        "sherpa-onnx-v1.13.4-linux-x64-static-lib.tar.bz2" => {
            "98b0e31996426f6e78244dbce1955548f2c64e8f01c4be75b85af7cdaa2e8d5c"
        }
        "sherpa-onnx-v1.13.4-linux-aarch64-static-lib.tar.bz2" => {
            "23b33616787cc949d5b1438e9794550f805e208a014c5c2245483207c58bbc0f"
        }
        "sherpa-onnx-v1.13.4-osx-x64-static-lib.tar.bz2" => {
            "2bda2c10b31a1cfc45d9f9e14bd4983743ec3779d309e42d99a6c8fa1689043f"
        }
        "sherpa-onnx-v1.13.4-osx-arm64-static-lib.tar.bz2" => {
            "57801db2bbb786a5d343f515a38ff210b401842338bdc804fa075312d1cd2404"
        }
        "sherpa-onnx-v1.13.4-win-x64-static-MT-Release-lib.tar.bz2" => {
            "d81bd1d25112540862d2387072e76b2b6843ef962918d6b5c7db5a19c6276b4c"
        }
        "sherpa-onnx-v1.13.4-linux-x64-shared-lib.tar.bz2" => {
            "3e7ce80379c938668f11157b1f54a0272b40972f618f445dcae71d122764d1fa"
        }
        "sherpa-onnx-v1.13.4-linux-aarch64-shared-cpu-lib.tar.bz2" => {
            "8993fd2ae4c435345f270231ae9af48def799638c8b1b06c0e48512e47c39e4d"
        }
        "sherpa-onnx-v1.13.4-osx-x64-shared-lib.tar.bz2" => {
            "24d37d744b9f4b6b6bff618ede6cede527d7c0073fcddeb554b5d13242a4544b"
        }
        "sherpa-onnx-v1.13.4-osx-arm64-shared-lib.tar.bz2" => {
            "995d38d323eef0bfbfe7432dcceffda91bbd95525a15fa64fed517ed368378b9"
        }
        "sherpa-onnx-v1.13.4-win-x64-shared-MT-Release-lib.tar.bz2" => {
            "f923e5eacb6bca83914d89cb31afa579e11eeaff9af39f8ead82ad19f44b2c9f"
        }
        _ => {
            return Err(format!(
                "No pinned SHA-256 is configured for sherpa-onnx archive: {archive_name}"
            )
            .into());
        }
    };
    Ok(sha256)
}

fn verify_archive_sha256(path: &Path, expected: &str) -> Result<(), DynError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "sherpa-onnx archive checksum mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn download_with_curl(url: &str, output: &Path) -> Result<bool, DynError> {
    let Some(parent) = output.parent() else {
        return Ok(false);
    };
    let temp = parent.join(format!(
        ".{}.{}.part",
        output
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("sherpa-onnx"),
        std::process::id()
    ));
    let status = match Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "30",
            "--retry",
            "3",
            "--retry-all-errors",
            "--retry-delay",
            "2",
            "--output",
        ])
        .arg(&temp)
        .arg(url)
        .status()
    {
        Ok(status) => status,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !status.success() {
        let _ = fs::remove_file(&temp);
        return Err(format!("curl failed to download {url}: {status}").into());
    }
    fs::rename(temp, output)?;
    Ok(true)
}

fn download_with_ureq(url: &str, output: &Path) -> Result<(), DynError> {
    let response = ureq::builder()
        .try_proxy_from_env(true)
        .build()
        .get(url)
        .call()?;
    let mut reader = response.into_reader();
    write_reader_atomically(&mut reader, output)
}

fn archive_name(
    link_mode: LinkMode,
    target_os: &str,
    target_arch: &str,
) -> Result<String, DynError> {
    let version = UPSTREAM_VERSION;
    let name = match (link_mode, target_os, target_arch) {
        (LinkMode::Static, "linux", "x86_64") => {
            format!("sherpa-onnx-v{version}-linux-x64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "linux", "aarch64") => {
            format!("sherpa-onnx-v{version}-linux-aarch64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "macos", "x86_64") => {
            format!("sherpa-onnx-v{version}-osx-x64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "macos", "aarch64") => {
            format!("sherpa-onnx-v{version}-osx-arm64-static-lib.tar.bz2")
        }
        (LinkMode::Static, "windows", "x86_64") => {
            format!("sherpa-onnx-v{version}-win-x64-static-MT-Release-lib.tar.bz2")
        }
        (LinkMode::Shared, "linux", "x86_64") => {
            format!("sherpa-onnx-v{version}-linux-x64-shared-lib.tar.bz2")
        }
        (LinkMode::Shared, "linux", "aarch64") => {
            format!("sherpa-onnx-v{version}-linux-aarch64-shared-cpu-lib.tar.bz2")
        }
        (LinkMode::Shared, "macos", "x86_64") => {
            format!("sherpa-onnx-v{version}-osx-x64-shared-lib.tar.bz2")
        }
        (LinkMode::Shared, "macos", "aarch64") => {
            format!("sherpa-onnx-v{version}-osx-arm64-shared-lib.tar.bz2")
        }
        (LinkMode::Shared, "windows", "x86_64") => {
            format!("sherpa-onnx-v{version}-win-x64-shared-MT-Release-lib.tar.bz2")
        }
        _ => {
            return Err(format!(
            "Unsupported target for sherpa-onnx prebuilt libs: os={target_os}, arch={target_arch}"
        )
            .into())
        }
    };

    Ok(name)
}

fn emit_shared_link_directives() {
    println!("cargo:rustc-link-lib=dylib=sherpa-onnx-c-api");
    println!("cargo:rustc-link-lib=dylib=onnxruntime");
}

fn emit_static_link_directives(target_os: &str) {
    for lib in SHERPA_ONNX_STATIC_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }

    match target_os {
        "linux" => {
            println!("cargo:rustc-link-lib=dylib=stdc++");
            println!("cargo:rustc-link-lib=dylib=m");
            println!("cargo:rustc-link-lib=dylib=pthread");
            println!("cargo:rustc-link-lib=dylib=dl");
        }
        "macos" => {
            println!("cargo:rustc-link-lib=dylib=c++");
            println!("cargo:rustc-link-lib=framework=Foundation");
        }
        _ => {}
    }
}

fn target_dir_from_out_dir(out_dir: &Path) -> Result<PathBuf, DynError> {
    if let Ok(explicit_target_dir) = env::var("CARGO_TARGET_DIR") {
        return Ok(PathBuf::from(explicit_target_dir));
    }

    if let Some(target_dir) = out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new("target")))
    {
        return Ok(target_dir.to_path_buf());
    }

    Ok(out_dir.to_path_buf())
}

fn emit_relative_rpath(target_os: &str) {
    match target_os {
        "linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        "macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}

fn profile_output_dirs() -> Result<[PathBuf; 2], DynError> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let profile = env::var("PROFILE")?;
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new(&profile)))
        .ok_or_else(|| {
            format!(
                "Could not locate Cargo profile directory from {}",
                out_dir.display()
            )
        })?
        .to_path_buf();

    Ok([profile_dir.clone(), profile_dir.join("examples")])
}

fn copy_unix_runtime_libs(lib_dir: &Path, target_os: &str) -> Result<(), DynError> {
    let runtime_libs: Vec<PathBuf> = fs::read_dir(lib_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .map(|name| match target_os {
                    "linux" => name.contains(".so"),
                    "macos" => name.ends_with(".dylib"),
                    _ => false,
                })
                .unwrap_or(false)
        })
        .collect();

    if runtime_libs.is_empty() {
        return Err(format!("No shared runtime libraries found in {}", lib_dir.display()).into());
    }

    let mut copy_plan = Vec::<(PathBuf, OsString)>::new();
    let mut planned_names = HashSet::<OsString>::new();

    for lib in runtime_libs {
        if !lib.exists() {
            continue;
        }

        let lib_name = lib
            .file_name()
            .ok_or_else(|| format!("Invalid runtime library path: {}", lib.display()))?
            .to_os_string();

        let source = fs::canonicalize(&lib).unwrap_or(lib.clone());
        if planned_names.insert(lib_name.clone()) {
            copy_plan.push((source.clone(), lib_name));
        }

        if let Some(source_name) = source.file_name() {
            let source_name = source_name.to_os_string();
            if planned_names.insert(source_name.clone()) {
                copy_plan.push((source.clone(), source_name));
            }
        }
    }

    if copy_plan.is_empty() {
        return Err(format!(
            "No usable shared runtime libraries found in {}",
            lib_dir.display()
        )
        .into());
    }

    for dest_dir in profile_output_dirs()? {
        fs::create_dir_all(&dest_dir)?;
        for (source, dest_name) in &copy_plan {
            let dest = dest_dir.join(dest_name);
            fs::copy(source, &dest)?;
        }
    }

    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut temp_name = path
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_else(|| OsString::from("tmp"));
    temp_name.push(".part");
    path.with_file_name(temp_name)
}

fn copy_file_atomically(src: &Path, dst: &Path) -> Result<(), DynError> {
    let temp_path = temp_path_for(dst);
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }
    fs::copy(src, &temp_path)?;
    fs::rename(&temp_path, dst)?;
    Ok(())
}

fn write_reader_atomically(reader: &mut dyn io::Read, dst: &Path) -> Result<(), DynError> {
    let temp_path = temp_path_for(dst);
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    {
        let mut file = File::create(&temp_path)?;
        io::copy(reader, &mut file)?;
        file.sync_all()?;
    }

    fs::rename(&temp_path, dst)?;
    Ok(())
}

fn copy_windows_runtime_dlls(lib_dir: &Path) -> Result<(), DynError> {
    let dlls: Vec<PathBuf> = fs::read_dir(lib_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension() == Some(OsStr::new("dll")))
        .collect();

    if dlls.is_empty() {
        println!(
            "cargo:warning=No runtime DLLs found in {}",
            lib_dir.display()
        );
        return Ok(());
    }

    let [profile_dir, examples_dir] = profile_output_dirs()?;
    for dest_dir in [profile_dir.clone(), examples_dir] {
        fs::create_dir_all(&dest_dir)?;
        for dll in &dlls {
            let dest = dest_dir.join(
                dll.file_name()
                    .ok_or_else(|| format!("Invalid DLL path: {}", dll.display()))?,
            );
            fs::copy(dll, &dest)?;
        }
    }

    println!(
        "cargo:warning=Copied Windows runtime DLLs to {} and {}/examples",
        profile_dir.display(),
        profile_dir.display()
    );

    Ok(())
}
