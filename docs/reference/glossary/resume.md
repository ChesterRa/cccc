# Term

`resume`

术语：`resume`

## Definition

In `cccc`, `resume` means recovering useful continuity for an actor or group.
That continuity may include CCCC-owned work-state recovery and, in some runtime
paths, native runtime session recovery.

在 `cccc` 里，`resume` 表示为 actor 或 group 恢复有用连续性。这种连续性既可能包含 CCCC 自己拥有的 work-state recovery，也可能在部分 runtime 路径里包含 native session recovery。

## Why It Exists

The term exists because continuity is valuable, but different recovery layers
must stay explicit.

这个术语存在，是因为连续性很有价值，但不同恢复层必须保持显式区分。

## What It Is Not

- It is not always native runtime session recovery.
- It is not proof that the old session is safe to continue.
- It is not equivalent to a successful `/status` readout alone.

## Canonical Scope

Runtime recovery semantics, operator expectations, status interpretation.

runtime 恢复语义、操作者预期、状态解释。

## Related Terms

- `status`
- `actor`
- `host_surface`

## Repo Usage Notes

- Current `cccc` docs should distinguish CCCC-owned recovery from runtime-native resume.
- Legacy wording that treats `resume` as if it always means native vendor session continuity should be treated as compatibility wording only.

## Status

Active with layered semantics

当前状态：有效，但带有分层语义

## Change Log

- `2026-03-21`: Added the local card to keep `resume` from collapsing into “native session recovery only”.
