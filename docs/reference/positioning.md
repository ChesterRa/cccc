# Positioning

This page defines what CCCC is, what it is not, and where it delivers the highest ROI.

## What CCCC Is

CCCC is a **local-first collaboration kernel** for multi-agent engineering work.

Core value:
- durable collaboration substrate (append-only ledger)
- unified control plane across Web/CLI/MCP/IM
- explicit message semantics for reliable coordination
- operationally manageable actor runtime model

## What CCCC Is Not

CCCC is not:
- a full visual workflow studio
- a generic enterprise BPM engine
- a replacement for your CI/CD system
- a substitute for deterministic batch orchestration platforms

CCCC should own collaboration state and control-plane semantics; other systems can own compute DAGs and business workflows.

## Ideal Adoption Scenarios

Use CCCC when you need:
- multiple coding agents with persistent shared context
- operationally reliable human-in-the-loop collaboration
- message-level accountability (read/ack/reply-required)
- remote/mobile operations via IM bridge

## Non-Ideal Scenarios

CCCC is likely a weak fit if you only need:
- single-agent local coding helper with no team coordination
- pure cron/batch ETL orchestration
- heavy GUI workflow modeling without code-first control

## Decision Checklist

Adopt CCCC if your answer is "yes" to most:

1. Do we need durable, replayable collaboration history?
2. Do we need multiple agents with role/recipient semantics?
3. Do we need one control plane across local and remote entry points?
4. Do we need operations-grade recovery/triage workflows?

## Integration Strategy

Recommended layering:

- CCCC: multi-agent collaboration state + control plane
- CI/CD: build/test/deploy lifecycle
- External schedulers: heavy workflow timing/orchestration
- IM gateway: remote ops and lightweight interventions

This separation keeps CCCC focused, composable, and maintainable.
