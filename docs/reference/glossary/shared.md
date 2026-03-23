# Term

`shared`

术语：`shared`

## Definition

`shared` is the workspace mode where an actor uses the group's authoritative
workspace as its execution workspace.

`shared` 是一种 workspace mode，表示 actor 使用 group 的 authoritative workspace 作为自己的 execution workspace。

## Why It Exists

This term exists to preserve the lightweight default path.

这个术语存在，是为了保住轻量默认路径。

## What It Is Not

- It is not a claim that no coordination risk exists.
- It is not equivalent to “all actors edit safely in parallel”.
- It is not a weaker form of authority; it is the default aligned path.

## Canonical Scope

Default operator path and execution policy.

默认操作者路径与执行策略。

## Related Terms

- `workspace_mode`
- `authoritative_workspace`
- `execution_workspace`
- `isolated`

## Repo Usage Notes

- Current product direction treats `shared` as the default recommended mode.
- `shared` is especially appropriate for proving collaboration, status, resume, and low-conflict runs first.

## Status

Recommended default

当前状态：推荐默认值

## Change Log

- `2026-03-21`: Added the local card so `shared` can be discussed as a first-class default mode instead of an unstated assumption.
