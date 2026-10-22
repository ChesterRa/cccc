# Native terminal turn observation

Actors normally running in a terminal have no authoritative turn state. Their
working state remains `waiting` / `pty_running_state_unknown`; terminal output
and quiet time do not determine whether a turn is active.

The optional per-Actor `turn_probe` setting accepts `off` (the default), `auto`,
or `hermes_turn_lease`. Set it with `cccc actor update <actor> --turn-probe auto`.
It is also a runtime-profile field; linked Actors use the profile's setting.
Changes apply on the **next start**, including changes back to `off`.

The first adapter supports Hermes CLI metadata at database schema version 31 on
Linux. `auto` selects it only for Hermes. Unsupported runtimes/platforms, named
Hermes profiles, unreadable or locked databases, unknown schemas, ambiguous
holders and process identity mismatches all preserve the existing fallback.
Managed sessions always take precedence. There is no output-based inference,
hook installation, provider launch, global setting, interval or idle threshold.

The ordinary activity publisher tick reads only lease and session metadata from
the provider's database, opened read-only with `query_only`. Queries have a
25 ms busy timeout, 50 ms progress budget and bounded rows/process ancestry.
Request handlers consume only the generation-validated cached result. No
message, prompt or response columns are selected.

An unexpired CLI lease counts as working only when its holder's PID namespace
and ancestry match the owned PTY generation. PID/start-time checks reject
reused identities; a lease acquired before that process was born cannot bind
it. Bindings are in memory and are discarded on a new terminal generation or
replacement of the provider's database file. Oversized metadata is rejected.

Idle requires a previously bound session row that is still open, a still-live
holder with the same process identity, and no active owned lease. A freshly
started Actor with no observed lease stays unknown until a real turn provides
that positive binding. An absent lease alone never establishes idle.

Successful observations fill the existing `effective_working_state`,
`effective_working_reason` (`turn_probe:hermes_turn_lease`) and timestamp fields.
The runtime state source remains `terminal`. A provider conversation identifier
is not a CCCC task identifier, so it does not fill `effective_active_task_id`.
Unknown preserves the existing fields. Probe-off adds no serialized Actor field
and makes no provider-state reads.
