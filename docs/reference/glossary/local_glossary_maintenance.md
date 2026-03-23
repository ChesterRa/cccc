# Local Glossary Maintenance

## Why CCCC Has A Local Glossary

`cccc` needs a repository-local glossary because some terms directly affect:

- CLI meaning
- runtime boundary interpretation
- status and resume interpretation
- operator understanding
- handoff consistency

`cccc` 需要自己的本地 glossary，因为有些术语会直接影响 CLI 语义、runtime 边界、`status` / `resume` 的理解，以及操作者和交接文档的一致性。

## Relationship To TwilightProject Upstream Glossary

- TwilightProject glossary remains the upstream cross-topic semantic source.
- `cccc` local glossary is the executable local subset used by this repository.
- Local wording should align with upstream direction, but local cards should only
  be added for terms that materially affect `cccc`.

## When To Add A New Local Card

Add a local card when a term materially affects:

- product meaning
- CLI meaning
- runtime truth or runtime boundary
- status / resume interpretation
- user or operator understanding
- repeated handoff or review language in this repo

## When To Sync Wording Back Upstream

Sync wording back upstream when:

- the term is no longer `cccc`-specific
- the same meaning is now reused across multiple topics or repositories
- upstream wording would benefit future cross-topic alignment
- local clarification reveals a real shared semantic gap

## Maintenance Rule

Update order:

1. update local glossary first
2. scan and align affected downstream `cccc` docs
3. update the changed docs' changelog entries

## Change Log

- `2026-03-21`: Added the first short maintenance note clarifying why `cccc` keeps a local glossary, how it relates to TwilightProject upstream glossary, and when local wording should be pushed back upstream.
