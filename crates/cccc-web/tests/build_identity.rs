const BINARY: &str = env!("CARGO_BIN_EXE_cccc-web");
const ARGUMENTS: &[&str] = &[];
const WEB: bool = true;

#[path = "../../cccc-core/tests/support/application_identity.rs"]
mod application_identity;
