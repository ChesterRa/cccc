//! Source identity compiled into the running process; never reads the checkout.
//! This is not a binary checksum or a protocol compatibility requirement.
pub fn current() -> serde_json::Value {
    serde_json::json!({"source_id": env!("CCCC_SOURCE_ID")})
}
