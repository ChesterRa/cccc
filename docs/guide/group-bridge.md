# Manual Group Bridge retired

CCCC Connect replaces manual Group Bridge for collaboration between instances
linked to the same account. See the [CCCC Connect guide](/guide/connect).

The old invitation, pairing, message/read/full grants and remote tool endpoints
are removed. Existing grants are not imported as account permissions. Original
messages and instance identity remain; pending deliveries are closed with their
confirmed, failed or unconfirmed outcome rather than retried through Connect.
An unreadable old receipt is retained for inspection and reported in the daemon
log; it does not keep a manual connection running.

Local cross-group messaging continues to work. Sharing a single Group with
another account is planned separately and is not available in this iteration.
