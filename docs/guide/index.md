# Guide

Use this section based on your role and goal.

## If You Are New to CCCC

- [Getting Started](/guide/getting-started/) for a 10-minute first setup
- [Web UI Quick Start](/guide/getting-started/web) if you prefer visual control
- [CLI Quick Start](/guide/getting-started/cli) if you prefer terminal-first workflow

Current recommended profile-backed start path:

```bash
cccc actor profile upsert --id starter-shared --name "Starter Shared" --runtime claude
cccc actor add starter --profile-id starter-shared
```

## If You Need Practical, High-ROI Patterns

- [Use Cases](/guide/use-cases) for production-like collaboration scenarios
- [Workflows](/guide/workflows) for common execution patterns
- [Best Practices](/guide/best-practices) for stable collaboration behavior

## If You Operate CCCC in Daily Work

- [Operations Runbook](/guide/operations) for triage, recovery, and upgrade flow
- [Web UI Guide](/guide/web-ui) for control-plane behavior
- [Capability Allowlist Baseline](/guide/capability-allowlist) for MCP/skill curation levels
- [IM Bridge](/guide/im-bridge/) for mobile/remote operations

## If You Need Troubleshooting

- [FAQ](/guide/faq)

## Core Concepts (Short Version)

- **Working Group**: the collaboration unit with durable history
- **Actor**: the live scheduled participant (foreman/peer)
- **Profile**: reusable actor runtime configuration and launch intent
- **Scope**: older compatibility wording for a directory context attached to a group
- **Ledger**: append-only collaboration event stream
- **Daemon**: single writer and source of operational truth

## Related Glossary

- [group](/reference/glossary/group)
- [actor](/reference/glossary/actor)
- [profile](/reference/glossary/profile)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)

## Change Log

- `2026-03-23`: Added a profile-backed start path to the guide landing page so high-level onboarding points to the current actor/profile setup model.
- `2026-03-23`: Added `profile` to the short core-concepts map so the main guide landing page no longer collapses reusable runtime configuration into actor-only wording.
