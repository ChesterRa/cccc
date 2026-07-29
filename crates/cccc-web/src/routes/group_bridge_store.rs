use cccc_core::HomeLayout;
use cccc_core::integration_state;
use serde_json::{Map, Value, json};
use sha2::Digest;
use std::io;
use uuid::Uuid;

const STORE_KEY: &str = "group_bridge";

pub struct BridgeStore<'a> {
    home: &'a HomeLayout,
}

impl<'a> BridgeStore<'a> {
    pub fn new(home: &'a HomeLayout) -> Self {
        Self { home }
    }

    pub fn load(&self) -> io::Result<Value> {
        self.import_legacy_if_changed()?;
        let mut value = integration_state::global_get(self.home, STORE_KEY)?;
        normalize(&mut value);
        Ok(value)
    }

    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
    ) -> io::Result<T> {
        self.import_legacy_if_changed()?;
        integration_state::global_update(self.home, STORE_KEY, |value| {
            normalize(value);
            change(value.as_object_mut().expect("bridge store initialized"))
        })
    }

    fn import_legacy_if_changed(&self) -> io::Result<()> {
        cccc_core::group_bridge_legacy::import_if_changed(self.home)
    }

    pub fn identity(&self) -> io::Result<Value> {
        let signing =
            cccc_core::group_bridge_identity::GroupBridgeIdentity::load_or_create(self.home)?;
        let digest = format!("{:x}", sha2::Sha256::digest(signing.peer_id.as_bytes()));
        let node_id = format!("node_{}", &digest[..24]);
        self.update(|state| {
            let identity = json!({
                "node_id":node_id,
                "peer_id":signing.peer_id
            });
            state.insert("identity".into(), identity.clone());
            Ok(identity.clone())
        })
    }
}

pub fn items<'a>(state: &'a Value, key: &str) -> &'a [Value] {
    state
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub fn items_mut<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("bridge section initialized")
}

pub fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_owned()
}

fn normalize(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
    let state = value.as_object_mut().expect("bridge store initialized");
    for key in [
        "invites",
        "requests",
        "trusts",
        "registrations",
        "outbounds",
        "deliveries",
    ] {
        state.entry(key).or_insert_with(|| json!([]));
    }
}
