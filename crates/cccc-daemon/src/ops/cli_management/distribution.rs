//! 固定官方来源的发行清单；下载、校验及解压仍交给 mise HTTP 后端。
use super::{install::concrete_version, process::Log};
use cccc_contracts::ActorRuntime;
use serde_json::Value;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) struct Release {
    pub version: String,
    url: String,
    sha256: String,
    bin_path: Option<&'static str>,
    binary: Option<&'static str>,
}

impl Release {
    pub fn config(&self, tool: &str) -> io::Result<String> {
        // JSON 字符串转义也是这些 ASCII 值所需的 TOML 基本字符串转义。
        let quoted = |value: &str| serde_json::to_string(value).map_err(io::Error::other);
        let mut text = format!(
            "[tools.{}]\nversion = {}\nurl = {}\nchecksum = {}\n",
            quoted(tool)?,
            quoted(&self.version)?,
            quoted(&self.url)?,
            quoted(&format!("sha256:{}", self.sha256))?,
        );
        if let Some(bin_path) = self.bin_path {
            text.push_str(&format!("bin_path = {}\n", quoted(bin_path)?));
        }
        if let Some(binary) = self.binary {
            text.push_str(&format!("bin = {}\n", quoted(binary)?));
        }
        Ok(text)
    }
}

fn hash(value: &str) -> io::Result<String> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::other(
            "官方发行清单缺少有效 SHA-256；拒绝无校验安装",
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn field<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("官方发行清单缺少字段 {key}")))
}

fn devin(manifest: &Value, os: &str, arch: &str) -> io::Result<Release> {
    let platform = match os {
        "linux" => "unknown-linux",
        "macos" => "apple-darwin",
        "windows" => "pc-windows",
        _ => return Err(io::Error::other("Devin 没有当前操作系统的发行包")),
    };
    let target = format!("{arch}-{platform}");
    let version = concrete_version(field(manifest, "version")?)?;
    let package = &manifest["platforms"][&target];
    let extension = if os == "windows" { "zip" } else { "tar.gz" };
    let expected =
        format!("https://static.devin.ai/cli/{version}/devin-{version}-{target}.{extension}");
    if field(package, "url")? != expected {
        return Err(io::Error::other("Devin 下载地址与官方固定版本路径不一致"));
    }
    Ok(Release {
        version,
        url: expected,
        sha256: hash(field(package, "sha256")?)?,
        bin_path: Some("bin"),
        binary: None,
    })
}

fn kiro(manifest: &Value, os: &str, arch: &str, musl: bool) -> io::Result<Release> {
    if os != "linux" {
        return Err(io::Error::other(
            "Kiro 隔离安装当前需要 Linux 归档；macOS DMG 适配尚未完成",
        ));
    }
    let version = concrete_version(field(manifest, "version")?)?;
    let suffix = if musl { "-musl" } else { "" };
    let path = format!("{version}/kirocli-{arch}-linux{suffix}.tar.gz");
    let package = manifest["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|package| package["download"].as_str() == Some(path.as_str()))
        })
        .ok_or_else(|| io::Error::other("Kiro 清单中没有当前平台的固定版本归档"))?;
    Ok(Release {
        version,
        url: format!("https://prod.download.cli.kiro.dev/stable/{path}"),
        sha256: hash(field(package, "sha256")?)?,
        bin_path: Some("kirocli/bin"),
        binary: None,
    })
}

fn droid(script: &str, os: &str, arch: &str, avx2: bool) -> io::Result<(String, String)> {
    // 只解析版本常量，绝不执行脚本（官方脚本可能终止已有 droid 进程）。
    let pattern =
        regex::Regex::new(r#"(?m)^VER="([0-9A-Za-z._+-]+)"\r?$"#).map_err(io::Error::other)?;
    let versions: Vec<_> = pattern.captures_iter(script).collect();
    if versions.len() != 1 {
        return Err(io::Error::other(
            "Droid 官方版本格式发生变化；未执行安装脚本",
        ));
    }
    let version = concrete_version(&versions[0][1])?;
    let platform = match os {
        "linux" => "linux",
        "macos" => "darwin",
        _ => return Err(io::Error::other("Droid 当前隔离发行路径不支持此操作系统")),
    };
    let architecture = match arch {
        "x86_64" if avx2 => "x64",
        "x86_64" => "x64-baseline",
        "aarch64" => "arm64",
        _ => return Err(io::Error::other("Droid 没有当前 CPU 架构的发行包")),
    };
    let url = format!(
        "https://downloads.factory.ai/factory-cli/releases/{version}/{platform}/{architecture}/droid"
    );
    Ok((version, url))
}

pub(super) fn resolve(runtime: ActorRuntime, log: &Log, stop: &AtomicBool) -> io::Result<Release> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .build()
        .map_err(io::Error::other)?;
    let fetch = |url: &str| -> io::Result<String> {
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "CLI 后台已停止"));
        }
        log.write("stage", &format!("读取官方发行元数据：{url}"))?;
        let response = client
            .get(url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(io::Error::other)?;
        if response.status().is_redirection() {
            return Err(io::Error::other("官方元数据地址发生重定向，请检查发行来源"));
        }
        let mut bytes = Vec::new();
        response.take(1_048_577).read_to_end(&mut bytes)?;
        if bytes.len() > 1_048_576 {
            return Err(io::Error::other("官方发行元数据超过 1 MiB"));
        }
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "CLI 后台已停止"));
        }
        String::from_utf8(bytes).map_err(io::Error::other)
    };
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match runtime {
        ActorRuntime::Devin => devin(
            &serde_json::from_str(&fetch("https://static.devin.ai/cli/current/manifest.json")?)?,
            os,
            arch,
        ),
        ActorRuntime::Kiro => kiro(
            &serde_json::from_str(&fetch(
                "https://prod.download.cli.kiro.dev/stable/latest/manifest.json",
            )?)?,
            os,
            arch,
            cfg!(target_env = "musl"),
        ),
        ActorRuntime::Droid => {
            #[cfg(target_arch = "x86_64")]
            let avx2 = std::is_x86_feature_detected!("avx2");
            #[cfg(not(target_arch = "x86_64"))]
            let avx2 = false;
            let (version, url) = droid(&fetch("https://app.factory.ai/cli")?, os, arch, avx2)?;
            let checksum = fetch(&format!("{url}.sha256"))?;
            Ok(Release {
                version,
                url,
                sha256: hash(checksum.split_whitespace().next().unwrap_or(""))?,
                bin_path: None,
                binary: Some("droid"),
            })
        }
        _ => Err(io::Error::other("此 Runtime 不使用官方 HTTP 发行清单")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manifests_pin_version_platform_and_checksum_without_executing_scripts() {
        let checksum = "a".repeat(64);
        let mut manifest = json!({"version":"1.2.3","platforms":{"x86_64-unknown-linux":{
            "url":"https://static.devin.ai/cli/1.2.3/devin-1.2.3-x86_64-unknown-linux.tar.gz", "sha256":checksum
        }}});
        let release = devin(&manifest, "linux", "x86_64").unwrap();
        assert!(
            release
                .config("http:cccc-devin")
                .unwrap()
                .contains("bin_path = \"bin\"")
        );
        manifest["platforms"]["x86_64-unknown-linux"]["url"] =
            json!("https://untrusted.invalid/archive.tar.gz");
        assert!(devin(&manifest, "linux", "x86_64").is_err());
        let manifest = json!({"version":"1.2.3","packages":[
            {"download":"1.2.3/kirocli-aarch64-linux.tar.gz", "sha256":checksum},
            {"download":"1.2.3/kirocli-aarch64-linux-musl.tar.gz", "sha256":checksum}
        ]});
        assert!(
            kiro(&manifest, "linux", "aarch64", true)
                .unwrap()
                .url
                .ends_with("-musl.tar.gz")
        );
        assert!(kiro(&manifest, "linux", "x86_64", false).is_err());
        let (version, url) = droid(
            "VER=\"1.2.3\"\npkill -KILL -x droid\n",
            "linux",
            "x86_64",
            false,
        )
        .unwrap();
        assert_eq!(version, "1.2.3");
        assert!(url.contains("/x64-baseline/"));
        let config = Release {
            version,
            url,
            sha256: checksum.clone(),
            bin_path: None,
            binary: Some("droid"),
        }
        .config("http:cccc-droid")
        .unwrap();
        assert!(!config.contains("bin_path"));
        assert!(config.contains("bin = \"droid\""));
        assert!(droid("VER=\"1.0\"\nVER=\"2.0\"", "linux", "x86_64", true).is_err());
        assert!(hash("invalid").is_err());
    }
}
