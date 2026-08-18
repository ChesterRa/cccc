"""The pinned DeepSeek Harness ACP release contract.

This module is the Python-side generated contract consumed by runtime
readiness checks.  Keep it data-only so setup, discovery and actor start all
read the same tuple.
"""

DEEPSEEK_DSH_PACKAGE = "@deepseek-ai/dsh"
DEEPSEEK_DSH_VERSION = "0.1.0-rc.6"
DEEPSEEK_ACP_PACKAGE = "@deepseek-ai/dsh-acp"
DEEPSEEK_ACP_VERSION = "0.1.0-rc.6"
DEEPSEEK_MCP_CLIENT_PACKAGE = "@deepseek-ai/dsh-mcp-client"
DEEPSEEK_MCP_CLIENT_VERSION = "0.1.0-rc.6"
DEEPSEEK_ACP_APP_PACKAGE = "@deepseek-ai/dsh-acp-demo"
DEEPSEEK_ACP_APP_VERSION = "0.1.0-rc.6"
DEEPSEEK_LLM_ADAPTER_PACKAGE = "@deepseek-ai/dsh-llm-deepseek"
DEEPSEEK_LLM_ADAPTER_VERSION = "0.1.0-rc.6"
DEEPSEEK_NODE_RANGE = "^22.19.0 || >=24.0.0"
DEEPSEEK_PROTOCOL_VERSION = 1
DEEPSEEK_ACP_SDK_VERSION = "0.25.1"

DEEPSEEK_PACKAGE_VERSIONS = (
    (DEEPSEEK_DSH_PACKAGE, DEEPSEEK_DSH_VERSION),
    (DEEPSEEK_ACP_PACKAGE, DEEPSEEK_ACP_VERSION),
    (DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION),
    (DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION),
    (DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION),
)


def is_canonical_profile_manifest(value: object) -> bool:
    """Validate the managed profile composition used by setup and preflight."""
    if not isinstance(value, dict):
        return False
    if value.get("name") != "dsh-profile-cccc-acp" or value.get("private") is not True:
        return False
    if value.get("ccccManaged") is not True:
        return False
    dependencies = value.get("dependencies")
    if not isinstance(dependencies, dict):
        return False
    if dependencies.get(DEEPSEEK_ACP_PACKAGE) != DEEPSEEK_ACP_VERSION:
        return False
    if dependencies.get(DEEPSEEK_MCP_CLIENT_PACKAGE) != DEEPSEEK_MCP_CLIENT_VERSION:
        return False
    if dependencies.get(DEEPSEEK_ACP_APP_PACKAGE) != DEEPSEEK_ACP_APP_VERSION:
        return False
    if dependencies.get(DEEPSEEK_LLM_ADAPTER_PACKAGE) != DEEPSEEK_LLM_ADAPTER_VERSION:
        return False
    profile = value.get("dsh", {}).get("profile") if isinstance(value.get("dsh"), dict) else None
    bundles = profile.get("bundles") if isinstance(profile, dict) else None
    return (
        isinstance(bundles, list)
        and len(bundles) == 2
        and all(isinstance(item, str) for item in bundles)
        and set(bundles) == {"@deepseek-ai/dsh-base", "@deepseek-ai/dsh-headless"}
    )
