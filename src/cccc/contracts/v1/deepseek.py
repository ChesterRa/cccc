"""The pinned DeepSeek Harness ACP release contract.

This module is the Python-side generated contract consumed by runtime
readiness checks.  Keep it data-only so setup, discovery and actor start all
read the same tuple.
"""

DEEPSEEK_RELEASE_VERSION = "0.1.0-rc.6"
# npm's prerelease semver ranges otherwise resolve the rc.6 dependency graph
# to rc.7 packages.  This cutoff is part of the compatibility contract.
DEEPSEEK_NPM_BEFORE = "2026-08-14T00:00:00Z"
DEEPSEEK_ACP_PACKAGE = "@deepseek-ai/dsh-acp"
DEEPSEEK_ACP_VERSION = DEEPSEEK_RELEASE_VERSION
DEEPSEEK_MCP_CLIENT_PACKAGE = "@deepseek-ai/dsh-mcp-client"
DEEPSEEK_MCP_CLIENT_VERSION = DEEPSEEK_RELEASE_VERSION
DEEPSEEK_ACP_APP_PACKAGE = "@deepseek-ai/dsh-acp-demo"
DEEPSEEK_ACP_APP_VERSION = DEEPSEEK_RELEASE_VERSION
DEEPSEEK_LLM_ADAPTER_PACKAGE = "@deepseek-ai/dsh-llm-deepseek"
DEEPSEEK_LLM_ADAPTER_VERSION = DEEPSEEK_RELEASE_VERSION
DEEPSEEK_NODE_RANGE = "^22.19.0 || >=24.0.0"
DEEPSEEK_PROTOCOL_VERSION = 1
DEEPSEEK_ACP_SDK_VERSION = "0.25.1"
DEEPSEEK_TURN_TIMEOUT_SECONDS = 300

DEEPSEEK_PACKAGE_VERSIONS = (
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
    return set(dependencies) == {package for package, _version in DEEPSEEK_PACKAGE_VERSIONS}
