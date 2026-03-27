# Term

`attach`

术语：`attach`

## Definition

In `cccc`, `attach` is the user-facing action that binds a project path to a
group as the group's authoritative workspace.

在 `cccc` 里，`attach` 是把某个项目路径绑定到 group 上、并把它设为该 group 的 authoritative workspace 的用户动作。

## Why It Exists

`attach` gives operators one explicit action for saying which project root the
group is currently anchored to.

它的存在，是为了给操作者一个明确动作，说明这个 group 当前到底锚定在哪个项目根路径上。

## What It Is Not

- It is not the same thing as actor launch.
- It is not the same thing as per-actor execution isolation.
- It is not a promise that every actor must run in its own separate workspace.

## Canonical Scope

Product, CLI, runtime anchor semantics.

产品、CLI、runtime 锚点语义。

## Related Terms

- `authoritative_workspace`
- `execution_workspace`
- `workspace_mode`
- `group`

## Repo Usage Notes

- In current `cccc` product direction, `attach` remains the authoritative path-setting action.
- Legacy wording such as `attach scope` may remain in older docs for compatibility, but glossary meaning wins.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Created the first local glossary card for `attach` and fixed its boundary relative to authoritative and execution workspace semantics.
