# Captured accepted-but-unread notice history

This five-record protocol projection contains a Mail, its notice, claimed and
accepted notice delivery, and later Mail to the same recipient. Both Mail
remain unread, unreplied and undelivered through the repeat assertions. The
source audit extends from the first Mail through 66 hours after the notice,
not merely to the last selected record. It finds no closing read (including
a later boundary), reply, delivery of either Mail, or Actor-generation change.
Only these two recipient Mail occur within the first 7,250 seconds after the
notice, covering the one-hour repeat and an additional interval. The longer
audit includes later arrivals, which are outside those assertion horizons.

The short-interval repeat and pruning cases use the captured prefix ending at
acceptance. Their multi-interval replay fits before the captured later Mail
arrives.
Their additional notices are outputs of the tested policy replayed with an
explicit issue-time translation, not claimed as captured historical repeats.

Event IDs, times, order, source references and delivery states are retained.
Identifiers and times are correlatable with the private source, not anonymous.
Group and Actor names use consistent aliases. Mail bodies are nonempty
`[redacted: N chars]` markers; N is the original character count. No body words
are retained. Private source hashes and per-record mappings are not published.

Tests bind the history to disposable Groups and translate its whole timeline,
preserving relative times. Later read/reply/delivery and lifecycle operations
are real controlled test actions. This replaces the earlier selection whose
later-boundary read invalidated its use as an unresolved repeat case; that
history is now retained separately in `mail_notice_read_history.json`.
