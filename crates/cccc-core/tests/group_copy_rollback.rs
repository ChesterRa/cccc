use cccc_core::group_copy;
use cccc_core::{GroupStore, HomeLayout};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[test]
fn failed_import_removes_the_partially_registered_group() {
    let source = tempfile::tempdir().expect("source");
    let source_home = HomeLayout::from_path(source.path().join("home")).expect("source home");
    let source_store = GroupStore::new(source_home).expect("source store");
    let group = source_store.create("source", "").expect("source group");
    let (package, _, _) = group_copy::export(&source_store, &group.group_id).expect("export");
    let broken = with_file_directory_conflict(&package);

    let target = tempfile::tempdir().expect("target");
    let target_home = HomeLayout::from_path(target.path().join("home")).expect("target home");
    let target_store = GroupStore::new(target_home).expect("target store");
    let error = group_copy::import(&target_store, &broken, "", "")
        .expect_err("conflicting paths must fail after registration");

    assert!(!error.to_string().contains("rollback_failed"));
    assert!(
        target_store
            .list()
            .expect("target groups")
            .iter()
            .all(|item| item.group_id != group.group_id)
    );
    assert!(
        !target_store
            .group_dir(&group.group_id)
            .expect("target group path")
            .exists()
    );
}

fn with_file_directory_conflict(package: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("archive");
    let mut entries = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("entry");
        if entry.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read entry");
        entries.insert(entry.name().to_owned(), data);
    }
    entries.insert("group/conflict".into(), b"file".to_vec());
    entries.insert("group/conflict/child".into(), b"child".to_vec());
    let files = entries
        .iter()
        .filter_map(|(name, data)| {
            name.strip_prefix("group/")
                .map(|relative| (relative.to_owned(), data.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&entries["manifest.json"]).expect("manifest");
    manifest["content_digest"] = serde_json::Value::String(content_digest(&files));
    entries.insert(
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest).expect("manifest bytes"),
    );

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut names = entries.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for name in names {
        writer.start_file(&name, options).expect("start entry");
        writer.write_all(&entries[&name]).expect("write entry");
    }
    writer.finish().expect("finish").into_inner()
}

fn content_digest(files: &HashMap<String, Vec<u8>>) -> String {
    let mut names = files.keys().collect::<Vec<_>>();
    names.sort();
    let mut digest = Sha256::new();
    for name in names {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(Sha256::digest(&files[name]));
        digest.update([b'\n']);
    }
    format!("{:x}", digest.finalize())
}
