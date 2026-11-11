const BINARY: &str = env!("CARGO_BIN_EXE_cccc");
const ARGUMENTS: &[&str] = &["mcp"];
const WEB: bool = false;

#[path = "../../cccc-core/tests/support/application_identity.rs"]
mod application_identity;
