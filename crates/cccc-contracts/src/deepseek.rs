//! Versioned DeepSeek Harness ACP compatibility contract.
//!
//! Keeping these values in the contracts crate prevents the Rust daemon and
//! its setup/preflight callers from silently drifting to a newer preview wire.

pub const DEEPSEEK_RELEASE_VERSION: &str = "0.2.2-rc.5";
/// The official launcher. Every `@deepseek-ai/dsh*` package it installs
/// carries the release version.
pub const DEEPSEEK_PACKAGE: &str = "@deepseek-ai/dsh";
/// npm cutoff after every rc.5 package was published (2026-11-18), so the
/// version ranges of the other dependencies resolve to the graph CCCC verified.
pub const DEEPSEEK_NPM_BEFORE: &str = "2026-11-19T00:00:00Z";
/// `engines.node` of the pinned launcher.
pub const DEEPSEEK_NODE_RANGE: &str = ">=24.9.0";
pub const DEEPSEEK_MODEL_ENV: &str = "DEEPSEEK_MODEL";
pub const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-flash";
pub const DEEPSEEK_PROTOCOL_VERSION: u64 = 1;
/// ACP SDK in the pinned release's dependency graph.
pub const DEEPSEEK_ACP_SDK_VERSION: &str = "1.4.0";
/// A turn fails when the provider sends nothing for this long. Every ACP
/// update or permission request restarts the window, so a turn that keeps
/// working is not cut off.
pub const DEEPSEEK_TURN_TIMEOUT_SECONDS: u64 = 300;
/// Output budget that preserves room for prompt and MCP tool context.
pub const DEEPSEEK_MAX_OUTPUT_TOKENS: u64 = 65536;
