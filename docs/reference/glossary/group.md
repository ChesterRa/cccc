# Term

`group`

术语：`group`

## Definition

A `group` is the core collaboration unit in `cccc`. It owns collaboration state,
actor membership, runtime attachment context, and durable history.

`group` 是 `cccc` 的核心协作单元。它承载协作状态、actor 成员、runtime 附着上下文和持久历史。

## Why It Exists

The term exists so collaboration can be scoped to one durable operational unit.

这个术语存在，是为了把协作限定到一个可持续、可运维的操作单元里。

## What It Is Not

- It is not only a chat room.
- It is not only a runtime session.
- It is not just a folder on disk.

## Canonical Scope

Kernel domain model and operator-facing collaboration semantics.

kernel 域模型与操作者协作语义。

## Related Terms

- `actor`
- `authoritative_workspace`
- `registry`
- `host_surface`

## Repo Usage Notes

- In `cccc`, a group remains the main unit of lifecycle, ledger history, and runtime ownership.
- Older wording such as `working group` remains compatible, but `group` is the concise local term.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Added the local card to anchor group meaning for product docs, CLI docs, and runtime notes.
