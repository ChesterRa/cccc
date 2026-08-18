//! Versioned DeepSeek Harness ACP compatibility contract.
//!
//! Keeping these values in the contracts crate prevents the Rust daemon and
//! its setup/preflight callers from silently drifting to a newer preview wire.

pub const DEEPSEEK_DSH_PACKAGE: &str = "@deepseek-ai/dsh";
pub const DEEPSEEK_DSH_VERSION: &str = "0.1.0-rc.6";
pub const DEEPSEEK_ACP_PACKAGE: &str = "@deepseek-ai/dsh-acp";
pub const DEEPSEEK_ACP_VERSION: &str = "0.1.0-rc.6";
pub const DEEPSEEK_MCP_CLIENT_PACKAGE: &str = "@deepseek-ai/dsh-mcp-client";
pub const DEEPSEEK_MCP_CLIENT_VERSION: &str = "0.1.0-rc.6";
pub const DEEPSEEK_ACP_APP_PACKAGE: &str = "@deepseek-ai/dsh-acp-demo";
pub const DEEPSEEK_ACP_APP_VERSION: &str = "0.1.0-rc.6";
pub const DEEPSEEK_LLM_ADAPTER_PACKAGE: &str = "@deepseek-ai/dsh-llm-deepseek";
pub const DEEPSEEK_LLM_ADAPTER_VERSION: &str = "0.1.0-rc.6";
pub const DEEPSEEK_NODE_RANGE: &str = "^22.19.0 || >=24.0.0";
pub const DEEPSEEK_PROTOCOL_VERSION: u64 = 1;
/// ACP SDK baseline locked for this preview wire contract.
pub const DEEPSEEK_ACP_SDK_VERSION: &str = "0.25.1";
