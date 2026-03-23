# Term

`profile`

术语：`profile`

## Definition

In `cccc`, `profile` means a reusable actor runtime profile: a stored runtime
configuration and secret-binding intent object that an actor can link to.

在 `cccc` 里，`profile` 指的是可复用的 actor runtime profile：它是一个可被 actor 关联的、持久保存的 runtime 配置与 secret-binding intent 对象。

## Why It Exists

This term exists so reusable runtime identity and launch intent do not have to
be collapsed into the live actor record itself.

这个术语存在，是为了把“可复用的 runtime 身份与启动意图”从 live actor 记录本身里分离出来，不必全部挤进 actor 这一层。

## What It Is Not

- It is not the live actor.
- It is not a guarantee that a runtime process is currently running.
- It is not the same thing as native session continuity proof.
- It is not a shell profile or browser profile unless the text says so explicitly.

## Canonical Scope

Reusable actor runtime configuration, launch intent, and profile-owned secrets.

可复用的 actor runtime 配置、启动意图，以及 profile-owned secrets。

## Related Terms

- `actor`
- `resume`
- `status`
- `execution_workspace`
- `registry`

## Repo Usage Notes

- Unqualified `profile` in `cccc` product/runtime docs should normally be read
  as `actor profile`.
- An actor is the scheduled collaboration participant; a profile is the reusable
  runtime configuration that actor may link to.
- `profile_id`, `profile_scope`, and `profile_owner` identify the linked
  profile, but they do not by themselves prove current live runtime state.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-23`: Added the local card so `profile` can become a first-class repo-local term instead of drifting between actor config, runtime identity, and unrelated shell/browser wording.
