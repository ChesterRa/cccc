use serde_json::Value;
use std::io;

use crate::settings;
use crate::{GroupStore, HomeLayout};

pub fn global_get(home: &HomeLayout, key: &str) -> io::Result<Value> {
    Ok(settings::load(home)?
        .extra
        .get(key)
        .cloned()
        .unwrap_or(Value::Null))
}

pub fn global_update<T>(
    home: &HomeLayout,
    key: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> io::Result<T> {
    crate::fs::with_exclusive_lock(&home.root().join("settings.yaml.lock"), || {
        let mut global = settings::load(home)?;
        let value = global.extra.entry(key.to_owned()).or_insert(Value::Null);
        let result = change(value)?;
        settings::save(home, &global)?;
        Ok(result)
    })
}

pub fn group_get(store: &GroupStore, group_id: &str, key: &str) -> io::Result<Value> {
    Ok(store
        .load(group_id)?
        .extra
        .get(key)
        .cloned()
        .unwrap_or(Value::Null))
}

pub fn group_update<T>(
    store: &GroupStore,
    group_id: &str,
    key: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> io::Result<T> {
    store.mutate(group_id, |group| {
        let value = group.extra.entry(key.to_owned()).or_insert(Value::Null);
        change(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn persists_global_and_group_namespaces() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        global_update(&home, "connectors", |value| {
            *value = json!([{"id":"one"}]);
            Ok(())
        })
        .expect("global update");
        assert_eq!(
            global_get(&home, "connectors").expect("global get")[0]["id"],
            "one"
        );

        let store = GroupStore::new(home).expect("store");
        let group = store.create("test", "").expect("group");
        group_update(&store, &group.group_id, "space", |value| {
            *value = json!({"provider":"notebooklm"});
            Ok(())
        })
        .expect("group update");
        assert_eq!(
            group_get(&store, &group.group_id, "space").expect("group get")["provider"],
            "notebooklm"
        );
    }
}
