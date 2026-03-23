# Term

`host_surface`

术语：`host_surface`

## Definition

A `host_surface` is a CCCC-owned readable surface that exposes host/runtime
truth to downstream consumers.

`host_surface` 是由 CCCC 拥有的可读取表面，用来向下游消费者暴露 host/runtime truth。

## Why It Exists

This term exists so downstream tools can consume runtime-owned facts without
confusing them with higher-level interpretation.

它的存在，是为了让下游工具可以消费 runtime-owned facts，而不会把它们和更高层解释混在一起。

## What It Is Not

- It is not a sidecar interpretation layer.
- It is not monitor projection logic.
- It is not business-specific workflow meaning.

## Canonical Scope

Machine-readable host/runtime observation surfaces.

机器可读的 host/runtime 观测表面。

## Related Terms

- `status`
- `resume`
- `registry`
- `group`
- `actor`

## Repo Usage Notes

- `cccc_runtime_capture_status` is one concrete host-surface example.
- Local docs should use `host_surface` when the point is readable host-owned truth, not downstream interpretation.

## Status

Active

当前状态：有效

## Change Log

- `2026-03-21`: Added the local card so host-owned readable surfaces can stop drifting into monitor or sidecar terminology.
