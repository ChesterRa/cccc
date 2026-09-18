#[path = "build_support/source.rs"]
mod source;

fn main() {
    let root = std::path::Path::new("../..");
    let inputs = source::inputs(root).expect("read CCCC source inputs");
    // New compiled modules also change an existing module/manifest. Do not watch
    // the whole crates directory: generated Web assets are a separate identity.
    // Resource directories also watch additions/removals, including nested files.
    for path in inputs.files.iter().chain(&inputs.resource_dirs) {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let id = source::fingerprint(root, &inputs).expect("fingerprint CCCC source");
    println!("cargo:rustc-env=CCCC_SOURCE_ID={id}");
}
