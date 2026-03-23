# Term

`authoritative_workspace`

术语：`authoritative_workspace`

## Definition

The `authoritative_workspace` is the project path that `cccc` treats as the
group's official workspace anchor.

`authoritative_workspace` 是 `cccc` 视为 group 官方工作区锚点的项目路径。

## Why It Exists

This term exists to keep authority separate from per-actor execution details.

这个术语存在的意义，是把 authority 和 actor 具体执行目录分开。

## What It Is Not

- It is not automatically every actor's current `cwd`.
- It is not replaced just because an actor uses an isolated workspace.
- It is not a bookkeeping convenience; it is a semantic anchor.

## Canonical Scope

Group-level product meaning and runtime truth.

group 级产品语义与 runtime truth。

## Related Terms

- `attach`
- `execution_workspace`
- `workspace_mode`
- `group`

## Repo Usage Notes

- Current product direction says `attach` defines the authoritative workspace.
- Older wording such as `scope`, `attached path`, or `project path` may still appear, but when they conflict, `authoritative_workspace` is the precise term.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Added the local card so `cccc` docs can distinguish authoritative path meaning from actor execution `cwd`.
