Vendored subset source:
- Upstream: https://github.com/agentscope-ai/ReMe
- Snapshot path used during integration: /tmp/reme_zip/ReMe-main
- Original upstream commit/tag: not recorded during the first import
- Imported for CCCC B1 memory integration (file-mode core)

Imported modules are intentionally partial and may include minimal compatibility edits
for CCCC runtime (dependency trimming, logging compatibility, and path adaptation).

Upgrade audit:
- 2026-06-15 compared CCCC's file-mode subset with upstream main
  f458566e2c826b7c84d16bf64ad806e7c7e768d5 and release v0.3.1.10.
- Decision: do not wholesale re-vendor ReMe. Upstream now includes a much larger
  application/service/vector/graph/job stack that overlaps CCCC's daemon, MCP,
  ledger, and runtime architecture.
- Adopted only the high-ROI local-search direction from newer ReMe4: dependency-free
  regex/CJK tokenization plus BM25-style keyword ranking in the existing local
  file store. No new external dependency or CCCC memory API change.
