# Term

`execution_workspace`

术语：`execution_workspace`

## Definition

The `execution_workspace` is the effective workspace path an actor runtime is
currently using for work execution.

`execution_workspace` 是 actor runtime 当前实际用于执行工作的有效工作区路径。

## Why It Exists

This term exists so that `cccc` can describe actor execution reality without
transferring authority away from the group anchor.

这个术语存在，是为了让 `cccc` 能描述 actor 的执行现实，同时不把 authority 从 group 锚点上转移走。

## What It Is Not

- It is not automatically the authoritative workspace.
- It is not proof that ownership moved to that path.
- It is not required to differ from the authoritative workspace.

## Canonical Scope

Actor runtime semantics and status interpretation.

actor runtime 语义与状态解释。

## Related Terms

- `authoritative_workspace`
- `workspace_mode`
- `shared`
- `isolated`
- `actor`

## Repo Usage Notes

- In `shared` mode, execution workspace usually equals authoritative workspace.
- In `isolated` mode, execution workspace may be a separate actor-local path, but that does not change the authoritative workspace.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Added the local card to stop `cwd`, `worktree path`, and authoritative project root from drifting into one ambiguous idea.
