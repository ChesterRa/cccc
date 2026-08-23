//! Versioned DeepSeek Harness ACP compatibility contract.
//!
//! Keeping these values in the contracts crate prevents the Rust daemon and
//! its setup/preflight callers from silently drifting to a newer preview wire.

pub const DEEPSEEK_RELEASE_VERSION: &str = "0.1.0-rc.6";
/// npm cutoff that keeps preview peer ranges on the validated rc.6 graph.
pub const DEEPSEEK_NPM_BEFORE: &str = "2026-08-14T00:00:00Z";
pub const DEEPSEEK_ACP_PACKAGE: &str = "@deepseek-ai/dsh-acp";
pub const DEEPSEEK_ACP_VERSION: &str = DEEPSEEK_RELEASE_VERSION;
pub const DEEPSEEK_MCP_CLIENT_PACKAGE: &str = "@deepseek-ai/dsh-mcp-client";
pub const DEEPSEEK_MCP_CLIENT_VERSION: &str = DEEPSEEK_RELEASE_VERSION;
pub const DEEPSEEK_ACP_APP_PACKAGE: &str = "@deepseek-ai/dsh-acp-demo";
pub const DEEPSEEK_ACP_APP_VERSION: &str = DEEPSEEK_RELEASE_VERSION;
pub const DEEPSEEK_LLM_ADAPTER_PACKAGE: &str = "@deepseek-ai/dsh-llm-deepseek";
pub const DEEPSEEK_LLM_ADAPTER_VERSION: &str = DEEPSEEK_RELEASE_VERSION;
pub const DEEPSEEK_NODE_RANGE: &str = "^22.19.0 || >=24.0.0";
pub const DEEPSEEK_PROTOCOL_VERSION: u64 = 1;
/// ACP SDK baseline locked for this preview wire contract.
pub const DEEPSEEK_ACP_SDK_VERSION: &str = "0.25.1";
pub const DEEPSEEK_TURN_TIMEOUT_SECONDS: u64 = 300;
/// Output budget that preserves room for prompt and MCP tool context.
pub const DEEPSEEK_MAX_OUTPUT_TOKENS: u64 = 65536;
