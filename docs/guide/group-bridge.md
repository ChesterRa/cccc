# Manual Group Bridge retired

CCCC Connect replaces manual Group Bridge for collaboration between instances
linked to the same account. See the [CCCC Connect guide](/guide/connect).

The old invitation, pairing, message/read/full grants and remote tool endpoints
are removed. Existing grants are not imported as account permissions. Original
messages and instance identity remain; pending deliveries are closed with their
confirmed, failed or unconfirmed outcome rather than retried through Connect.
An unreadable old receipt is retained for inspection and reported in the daemon
log; it does not keep a manual connection running.

Local cross-group messaging continues to work. To connect a specific Group with
another member, use **⋮ → Group connections** in the Group’s sidebar menu and confirm the
invitation on the account website. Both members select their own Group; old
manual grants are never imported. See the [Connect guide](/guide/connect).

For standalone connections without the account service, use **Group connections →
Direct connection**. This uses the current scoped messaging and receipt engine,
not the retired manual permissions or remote-tool endpoints. See
[Direct connections](/guide/connect#direct-connections-without-an-account).
