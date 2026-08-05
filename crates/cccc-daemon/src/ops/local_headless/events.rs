use cccc_contracts::utc_now;
use cccc_core::{GroupStore, HomeLayout};
use fs2::FileExt;
use serde_json::{Map, Value, json};
use std::fs::OpenOptions;
use std::io::{self, Write};
use uuid::Uuid;

pub(super) fn append(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    kind: &str,
    data: Map<String, Value>,
) -> io::Result<()> {
    let store = GroupStore::new(home.clone())?;
    let directory = store.state_dir(group_id)?.join("headless");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("events.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    file.lock_exclusive()?;
    let result = (|| {
        serde_json::to_writer(
            &mut file,
            &json!({
                "id": Uuid::new_v4().simple().to_string(),
                "ts": utc_now(),
                "group_id": group_id,
                "actor_id": actor_id,
                "type": kind,
                "data": data,
            }),
        )
        .map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.flush()
    })();
    let unlock = FileExt::unlock(&file);
    result.and(unlock)
}
