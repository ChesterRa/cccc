mod source;

use std::io;
use std::path::Path;

/// Shared by executable packages so their source inputs and watch lists agree.
pub fn emit(root: &Path) -> io::Result<()> {
    let inputs = source::inputs(root)?;
    // Module declarations/manifests reveal new Rust modules. Resource directory
    // watches also cover additions and removals without watching build outputs.
    for path in inputs.files.iter().chain(&inputs.resource_dirs) {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    println!(
        "cargo:rustc-env=CCCC_SOURCE_ID={}",
        source::fingerprint(root, &inputs)?
    );
    Ok(())
}
