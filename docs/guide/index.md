# Guide

CCCC coordinates coding agents as a durable group chat: messages are routed, read state is visible, operations are controlled through one daemon, and the same group can be reached from Web UI, CLI, MCP, and IM bridges.

Use this section based on what you are trying to do next.

## If You Are New to CCCC

- [Getting Started](/guide/getting-started/) for a 10-minute first setup
- [Web UI Quick Start](/guide/getting-started/web) if you prefer visual control
- [CLI Quick Start](/guide/getting-started/cli) if you prefer terminal-first workflow

## If You Need Practical, High-ROI Patterns

- [Use Cases](/guide/use-cases) for production-like collaboration scenarios
- [Workflows](/guide/workflows) for common execution patterns
- [Best Practices](/guide/best-practices) for stable collaboration behavior

## If You Operate CCCC in Daily Work

- [Operations Runbook](/guide/operations) for triage, recovery, and upgrade flow
- [Web UI Guide](/guide/web-ui) for control-plane behavior
- [Supported Runtimes](/guide/runtimes) for Claude Code, Codex, ChatGPT Web, Grok, Kimi, and other actor runtimes
- [Group Bridge](/guide/group-bridge) for trusted cross-group messaging and remote access
- [Capability Allowlist Baseline](/guide/capability-allowlist) for MCP/skill curation levels
- [Contributor Quality Gates](/guide/quality-gates) for local checks, CI boundaries, and stable Python shards
- [ChatGPT Web Model Runtime](/guide/web-model-runtime) for MCP-capable ChatGPT GPT-5.x setup
- [IM Bridge](/guide/im-bridge/) for mobile/remote operations

## If You Need Troubleshooting

- [FAQ](/guide/faq)

## Core Concepts (Short Version)

- **Working Group**: the collaboration unit with durable history
- **Actor**: an agent runtime session (foreman/peer)
- **Scope**: a directory context attached to a group
- **Ledger**: append-only collaboration event stream
- **Daemon**: single writer and source of operational truth
- **Group Bridge**: an explicit trust edge between local-first CCCC groups
