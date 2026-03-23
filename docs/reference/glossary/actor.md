# Term

`actor`

术语：`actor`

## Definition

An `actor` is a scheduled collaboration participant inside a `cccc` group.

`actor` 是 `cccc` group 内部的一个可调度协作参与者。

## Why It Exists

The term exists so runtime identity, collaboration identity, and operator-facing
coordination can be expressed with one stable unit.

它的存在，是为了让 runtime 身份、协作身份和操作者协调对象能通过一个稳定单元表达出来。

## What It Is Not

- It is not merely a terminal tab.
- It is not only a runtime process.
- It is not interchangeable with a user.

## Canonical Scope

Runtime domain, collaboration domain, CLI and Web operator surfaces.

runtime 域、协作域，以及 CLI / Web 操作面。

## Related Terms

- `group`
- `profile`
- `execution_workspace`
- `resume`
- `status`

## Repo Usage Notes

- `actor_id` is the stable identity key.
- Display title and runtime session identity may change, but the actor term refers to the scheduled participant itself.
- An actor may link to a reusable `profile`, but actor and profile are not the
  same semantic object.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Added the local card so actor identity can stop drifting between UI wording, runtime notes, and handoff prose.
- `2026-03-23`: Clarified that a live actor can link to a reusable profile without collapsing the two terms into one.
