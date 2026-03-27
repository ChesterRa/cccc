# CCCC Local Glossary

This directory is the repository-local glossary for `cccc`.

这个目录是 `cccc` 仓库内部的本地术语表。

## Purpose

The local glossary exists so that `cccc` product docs, CLI docs, runtime notes,
and handoff docs can share one repository-local semantic source for the terms
that materially affect operator understanding and product meaning.

本地 glossary 的作用，是给 `cccc` 的产品文档、CLI 文档、runtime 说明和交接文档提供一套仓库内部统一可执行的语义来源。

## Governance Boundary

- The upstream cross-topic semantic source remains:
  - `/Users/glennxu/workspace/Twilight/V4/TwilightProject/docs/topics/glossary/README.md`
  - `/Users/glennxu/workspace/Twilight/V4/TwilightProject/docs/topics/cccc_product_and_requirement_evolution/README.md`
  - `/Users/glennxu/workspace/Twilight/V4/TwilightProject/docs/topics/cccc_product_and_requirement_evolution/current_product_direction.md`
  - `/Users/glennxu/workspace/Twilight/V4/TwilightProject/docs/topics/cccc_product_and_requirement_evolution/workspace_mode_shared_vs_isolated_notes.md`
- This local glossary only keeps the subset of terms that materially affect `cccc`.
- Local cards should align with upstream direction, but should not blindly copy
  all upstream glossary files.
- If a local glossary card conflicts with older `cccc` prose, the glossary card
  wins.

## Current Card Set

- [attach](./attach.md)
- [authoritative_workspace](./authoritative_workspace.md)
- [execution_workspace](./execution_workspace.md)
- [workspace_mode](./workspace_mode.md)
- [shared](./shared.md)
- [isolated](./isolated.md)
- [group](./group.md)
- [actor](./actor.md)
- [profile](./profile.md)
- [resume](./resume.md)
- [status](./status.md)
- [registry](./registry.md)
- [host_surface](./host_surface.md)

## Authoring Rules

- File names must be English only.
- Content should be bilingual: English plus Chinese.
- Each card should define meaning and boundary, not become an implementation notebook.
- If old wording must remain for history, label it as `legacy` or `compatibility wording`.
- When a card changes, downstream `cccc` docs using that term should be reviewed.

## Related Note

- [local_glossary_maintenance.md](./local_glossary_maintenance.md)

## Change Log

- `2026-03-21`: Created the bounded repo-local glossary root for the first set of `cccc` terms that directly affect operator understanding, CLI semantics, runtime boundaries, and resume/status interpretation.
- `2026-03-23`: Added `profile` to the canonical local card set so reusable runtime identity and launch intent are explicit in repo-local semantics.
