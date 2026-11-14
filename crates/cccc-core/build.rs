use std::fs;
use std::io;
use std::path::Path;

fn main() -> io::Result<()> {
    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("../../resources/capability_skills");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;
    files.sort();
    let mut generated = String::from("const SKILL_FILES: &[(&str, &str)] = &[\n");
    for (name, path) in files {
        generated.push_str(&format!("({name:?}, include_str!({path:?})),\n"));
    }
    generated.push_str("];\n");
    fs::write(
        Path::new(&std::env::var("OUT_DIR").expect("output directory"))
            .join("capability_skill_files.rs"),
        generated,
    )
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, String)>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::other("capability assets must not be symlinks"));
        }
        let path = entry.path();
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() {
            let name = path
                .strip_prefix(root)
                .expect("asset under root")
                .to_str()
                .ok_or_else(|| io::Error::other("capability asset path must be UTF-8"))?
                .replace('\\', "/");
            let path = path
                .to_str()
                .ok_or_else(|| io::Error::other("capability asset path must be UTF-8"))?;
            files.push((name, path.into()));
        }
    }
    Ok(())
}
