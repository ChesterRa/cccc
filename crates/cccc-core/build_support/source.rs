use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// Content-based, independent of Git, checkout paths, timestamps and build outputs.
// Web assets have a separate fingerprint in cccc-web.
pub struct Inputs {
    pub files: Vec<PathBuf>,
    pub resource_dirs: Vec<PathBuf>,
}

pub fn inputs(root: &Path) -> io::Result<Inputs> {
    fn walk(root: &Path, path: &Path, resources: bool, inputs: &mut Inputs) -> io::Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                let resource_dir = entry.file_name() == "resources";
                if resources
                    || resource_dir
                    || !matches!(
                        entry.file_name().to_str(),
                        Some("target" | "tests" | "assets")
                    )
                {
                    if resource_dir && !resources {
                        inputs
                            .resource_dirs
                            .push(path.strip_prefix(root).expect("source root").to_owned());
                    }
                    walk(root, &path, resources || resource_dir, inputs)?;
                }
            } else if resources
                || matches!(
                    path.extension().and_then(|v| v.to_str()),
                    Some("rs" | "toml")
                )
            {
                inputs
                    .files
                    .push(path.strip_prefix(root).expect("source root").to_owned());
            }
        }
        Ok(())
    }
    let mut inputs = Inputs {
        files: vec![PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")],
        resource_dirs: vec![PathBuf::from("resources")],
    };
    walk(root, &root.join("crates"), false, &mut inputs)?;
    walk(root, &root.join("resources"), true, &mut inputs)?;
    inputs.files.sort();
    inputs.resource_dirs.sort();
    Ok(inputs)
}

pub fn fingerprint(root: &Path, inputs: &Inputs) -> io::Result<String> {
    let mut hash = Sha256::new();
    for path in &inputs.files {
        let name = path.to_string_lossy().replace('\\', "/");
        let bytes = fs::read(root.join(path))?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}
