# Captured later-boundary read before the repeat

This six-record projection retains the earlier Mail, notice, claimed/accepted
notice delivery, later Mail, and the recipient's actual read. The read's
boundary is the later Mail and therefore closes both Mail. It occurs before
the one-hour repeat point. This is a stop case, not an accepted-but-unread
history through that interval. The earlier five-record selection omitted the
read and could not support that claim.

The source audit extends beyond the expected repeat plus one interval and
confirms the first closing fact is this captured read. Replay also restores
its cursor boundary through the production read transaction: appending a
ledger read record alone does not restore the cursor file. The generated
cursor transaction is a controlled replay operation, not a captured event.

Event IDs, times, order and protocol facts are retained. IDs and times are
correlatable with the private source. Identities use aliases and bodies use
nonempty `[redacted: N chars]` markers. Original bodies and private provenance
are not published. Tests bind this history to disposable Groups and translate
the whole timeline while preserving relative times and the later boundary.
