# Term

`registry`

术语：`registry`

## Definition

In `cccc`, `registry` is a bookkeeping and lookup surface used to index durable
objects such as groups or defaults.

在 `cccc` 里，`registry` 是一个用于索引 group、默认项等持久对象的记账与查询面。

## Why It Exists

It exists so the system can find and explain known objects without rescanning
every other state file each time.

它存在，是为了让系统能在不每次全量重扫其他状态文件的前提下，定位并解释已知对象。

## What It Is Not

- It is not the new source of truth.
- It is not a second control plane.
- It is not authority transfer away from group state or runtime truth.

## Canonical Scope

Bookkeeping, lookup, diagnostics.

记账、查询、诊断。

## Related Terms

- `group`
- `host_surface`
- `workspace_mode`
- `isolated`

## Repo Usage Notes

- Current product direction is conservative: registry is useful, but should stay secondary to authoritative state and runtime truth.
- If future workspace registry exists, this glossary meaning still applies.

## Status

Active but secondary

当前状态：有效，但属次级语义面

## Change Log

- `2026-03-21`: Added the local card to keep registry discussion aligned with the product rule that registry must not become the new truth root.
