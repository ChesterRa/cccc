# Term

`isolated`

术语：`isolated`

## Definition

`isolated` is the workspace mode where an actor's execution workspace is kept
separate from the group's authoritative workspace.

`isolated` 是一种 workspace mode，表示 actor 的 execution workspace 与 group 的 authoritative workspace 保持分离。

## Why It Exists

It exists for bounded cases where actor-local isolation is useful, such as
parallel code changes or experimental lanes.

它的存在，是为了支持 actor 本地隔离确实有价值的受限场景，例如并发改代码或实验性工作区。

## What It Is Not

- It is not the new source of truth.
- It is not required for every actor.
- It is not permission to silently reinterpret group authority.

## Canonical Scope

Optional execution policy and advanced operator capability.

可选执行策略与高级操作者能力。

## Related Terms

- `workspace_mode`
- `shared`
- `execution_workspace`
- `registry`

## Repo Usage Notes

- Current product direction treats `isolated` as an explicit optional capability, not the main path.
- If future worktree support arrives, the glossary meaning still stays semantic rather than implementation-specific.

## Status

Optional advanced direction

当前状态：可选增强方向

## Change Log

- `2026-03-21`: Added the local card to prevent isolated execution from being misread as authority transfer.
