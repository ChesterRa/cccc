# Term

`workspace_mode`

术语：`workspace_mode`

## Definition

`workspace_mode` is the policy term that explains how `cccc` resolves an
actor's execution workspace relative to the authoritative workspace.

`workspace_mode` 是一个策略术语，用来说明 `cccc` 应该如何把 actor 的 execution workspace 相对于 authoritative workspace 进行解析。

## Why It Exists

It exists so optional workspace isolation can be explicit instead of silently
rewriting the default path.

它的存在，是为了让可选的工作区隔离成为显式能力，而不是悄悄改写默认路径。

## What It Is Not

- It is not a replacement for `attach`.
- It is not itself a registry.
- It is not proof that isolated workspaces should become the default path.

## Canonical Scope

Product policy, operator-facing configuration, status explanation.

产品策略、操作者配置、状态解释。

## Related Terms

- `attach`
- `authoritative_workspace`
- `execution_workspace`
- `shared`
- `isolated`

## Repo Usage Notes

- Current product direction recommends default `workspace_mode = shared`.
- `workspace_mode` should stay explicit and optional rather than becoming hidden automation.

## Status

Proposed-active direction

当前状态：建议中的有效方向

## Change Log

- `2026-03-21`: Added the first local definition so future docs can discuss shared vs isolated execution without weakening attach semantics.
