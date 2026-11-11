Captured 2026-11-11 from a live group's `runtime_states` record. Actor,
group and event ids are redacted; status, turn fields and the write time are
retained. The record has no `last_turn_end`, so it is the base for tests that
write a status through the real op. Test mutations are explicit operations on
this recorded state.
