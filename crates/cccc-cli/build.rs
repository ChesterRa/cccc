#[path = "../cccc-core/build_support/mod.rs"]
mod source_id;

fn main() {
    source_id::emit(std::path::Path::new("../..")).expect("fingerprint CCCC application source");
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("-windows-msvc")) {
        // The unoptimized async command dispatcher can exhaust the default
        // 1 MiB main stack before it parses even --version.
        println!("cargo:rustc-link-arg-bin=cccc=/STACK:8388608");
    }
}
