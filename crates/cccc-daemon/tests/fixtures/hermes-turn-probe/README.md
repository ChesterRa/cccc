# Hermes turn-lease capture

`during.sqlite3` is a metadata-only projection captured read-only from a real
Hermes CLI state database on 2026-10-22 (schema version 31). It retains the
original DDL for `schema_version`, `session_turn_leases` and `sessions`, all 30
observed lease rows, and the 30 corresponding session rows. Only `id`, `source`,
`started_at` and `ended_at` were selected from sessions; all remaining columns
are NULL or the schema's defaults. No message tables, bodies, prompts,
credentials, environment values, actor/group identifiers or filesystem paths
are included. No provider-state row was generated for these tests.

`capture.json` records the capture clock, original kernel PID/start/namespace
metadata and ancestry for the live CLI holders, and the database digest. Numeric
process identities, timestamps and provider session identifiers are unmodified.
The process snapshot is replayed through a read-only test view: tests never
contact the original processes or provider installation.

Expiry tests advance the replay clock past the captured provider expiry; this
does not create a provider record or claim that a real turn ended at that time.
Negative controls delete a table/row or change a schema version on disposable
copies of this capture. They are deliberately damaged captures, not additional
provider observations. Database contention uses an exclusive lock on a copy.
The original session DDL references content tables that are intentionally absent
from this projection. Mutation controls disable foreign-key enforcement on their
disposable copy; the adapter performs only read queries.
