# Captured notice history

`mail_notice_history.json` is a structural projection of five ordered ledger
records captured on 2026-10-22: a concrete-recipient Mail, its notice, the
notice's claimed and accepted delivery records, and later Mail to that recipient.
Event IDs, timestamps, order, notice source references and delivery states are
retained. The complete source window was checked for reads, replies, manual
delivery, generation changes, replacement notices and resumes; no relevant
closing or replacement fact is omitted. Group and Actor identifiers use consistent aliases. Private Mail text
and unrelated fields are omitted.

Tests explicitly bind the projection to a disposable Group and translate the
whole timestamp series relative to its captured notice, preserving all time
differences. Daemon tests may bind the notice's generation to their newly created
recipient. Subsequent reads, replies, manual deliveries and generation changes
are controlled test operations; they are not claimed as captured history.
