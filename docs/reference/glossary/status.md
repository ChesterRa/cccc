# Term

`status`

术语：`status`

## Definition

`status` is the operator-facing state surface that reports current runtime,
configuration, or lifecycle reality as honestly as the available evidence allows.

`status` 是面向操作者的状态读取面，用来在现有证据允许的前提下尽量诚实地报告 runtime、配置或生命周期的当前现实。

## Why It Exists

The term exists because operators need one explainable way to understand what
the system currently believes is true.

它的存在，是因为操作者需要一个可解释的读取面，来理解系统当前认为哪些事实成立。

## What It Is Not

- It is not automatically proof of every deeper capability.
- It is not the same as control-plane ownership.
- It is not a substitute for richer diagnostics when the system is ambiguous.

## Canonical Scope

Operator semantics, runtime observation, CLI and helper interpretation.

操作者语义、runtime 观测、CLI 与 helper 解释。

## Related Terms

- `resume`
- `host_surface`
- `registry`

## Repo Usage Notes

- Current `cccc` direction prefers status surfaces that are explicit about present, missing, partial, and inferred evidence.
- Older prose that treats a status line as full proof of session resumability should be treated as legacy shorthand.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Added the local card so `status` can consistently mean an evidence-bound observation surface rather than a vague success label.
