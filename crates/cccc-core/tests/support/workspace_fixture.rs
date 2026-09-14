use cccc_core::{GroupDoc, GroupStore, HomeLayout, Scope, group_scope};

pub struct Fixture {
    _temp: tempfile::TempDir,
    pub repo: std::path::PathBuf,
    pub group: GroupDoc,
}

pub fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("src");
    std::fs::write(repo.join("src/lib.rs"), "fn main() {}\n").expect("lib");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = GroupStore::new(home).expect("groups");
    let group = groups.create("workspace", "").expect("group");
    group_scope::attach(
        &groups,
        &group.group_id,
        Scope {
            scope_key: "scope_repo".into(),
            // Canonicalize so the fixture matches what the group store would persist on macOS,
            // where /var is a symlink to /private/var.
            url: repo
                .canonicalize()
                .expect("canonicalize")
                .to_string_lossy()
                .into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let group = groups.load(&group.group_id).expect("reload");
    Fixture {
        _temp: temp,
        repo,
        group,
    }
}
