# CCCC Connect

CCCC Connect brings instances linked to the same membership account into one
workspace while each instance keeps its own Groups, files, Actors and history.
Local CCCC use continues to work without an account. For collaboration between
selected Groups without the account service, see
[Direct connections](#direct-connections-without-an-account).

## Link instances

In each instance, open **Settings → Account** and link the same account. Connect
background discovery and Group/Actor communication are enabled by that binding;
there is no Network to create or Group pair to configure. Link only instances
whose Groups may communicate with every other instance on the account.

Linking from trusted localhost administration prepares the first administrator
Access Token and signs that browser in automatically. Existing Tokens remain
unchanged; remote first setup still requires the local bootstrap code. In
**Settings → Account**, edit **Instance name** to identify the workstation. This
is the same instance name shown on the account website. New device grants use
the machine hostname as an initial name when available; reconnecting does not
overwrite an existing name. The editor identifies the instance and its address.
The sidebar marks the current instance and nests each Group under its instance.
The Account menu shows the account linked to this instance when the account
service supplies its identity; this is separate from the Web Access Token in use.
Duplicate instance names receive a short identifier only in the sidebar label.

Each peer needs a reachable HTTPS Web origin. **Remote Access** configures the
managed remote-access route; an explicitly configured reachable route can also
be used. Account linkage alone does not prove that a route is reachable. Account
settings distinguish directory confirmation, route availability and tunnel status.
Outdated clients receive an upgrade requirement rather than a partial connection.

## Open another instance

Sign into the current Web instance as an administrator. Other linked instances
appear in the sidebar. Choose an instance and enter **that instance's own admin
Access Token** to open its Groups. The current instance's Token does not unlock
other instances. Each remote view is the target's native Web UI, including its
messages and terminals; credentials and data stay with the target origin.

Previously opened remote Group lists remain in the sidebar when you select a
local Group or another instance. Use each instance's arrow to collapse its list.
A temporarily failed refresh keeps the last-known list with an explanatory
status. Expired confirmation disables opening instances; fresh authorization is
still required. An explicit unlink or access revocation clears that navigation.
Inactive lists are saved navigation, not live status: their frames and terminals
close, and opening a Group rechecks access.

A restricted Token keeps the workspace in a single-instance view. It does not
change the background account/device communication grant. Browser login is
reused within its partition for the same entry site and target. Distinct HTTPS
hostnames are required for embedded instance views; different ports on the same
hostname do not isolate cookies. Open cross-instance microphone features in the
target's standalone page.

## Agent collaboration

In an administrator Web session, type `#` in the message composer to find local
and connected Groups. Remote entries show **Group · Instance**; their cached
information may be outdated and does not prove they are online. Selecting an
entry adds a reference for your current recipients—it does **not** change `To`
or send your text to that Group. For example, ask your local Agent to coordinate
with `#Team · Mac`; the reference carries the exact remote identity so the Agent
can discover and contact it through Connect. `#Group @` also offers that Group's
cached enabled Actors. Switching local Groups preserves references with their
drafts. Restricted Web sessions continue to offer only local Groups.

The same tools serve account connections and Direct connections. They are part
of the ordinary Actor's core MCP catalog; no capability installation or Access
Token is needed for Agent messaging.

1. Call `cccc_connect()` in the local Group. Check both `instances` (same account)
   and `external_groups` (connections granted to this specific Group).
2. For a same-account instance, call `cccc_connect(instance_id="...")`. For an
   `external_groups` entry, also set `target_group_id` to that entry's `group_id`.
   The result includes the Group's Actors. Follow `next` with `after` if present.
   Instance/Group names are labels; use the returned IDs to address messages.
3. Send with `cccc_message_send(dst_instance_id="...", dst_group_id="...",
   to=["@foreman"], text="...", insight="...", mode="send")`. Use an Actor ID
   for a specific recipient. `mode="send"` requests immediate delivery; the
   MCP default `mode="mail"` leaves Mail for the recipient to read.
   `cccc_file(action="send")` supports the same qualified destination for small
   attachments.
4. On receipt, call `cccc_message_reply(event_id="...", text="...", insight="...")`
   using the **local Event ID** in the incoming message. Omit `to` to reply to
   the original sender. Replies use the existing connection automatically.

`cccc_group` and `cccc_actor` manage local Groups; they do not enumerate or
administer remote instances. Cached directory freshness describes metadata age,
not live reachability. An empty `instances` list does not imply there are no
Direct connections: check `external_groups` too.

The CLI provides the same discovery and message routes:

```bash
cccc connect --group LOCAL_GROUP
cccc connect --group LOCAL_GROUP --instance REMOTE_INSTANCE --target-group REMOTE_GROUP
cccc send "Hello" --group LOCAL_GROUP --dst-instance REMOTE_INSTANCE \
  --dst-group REMOTE_GROUP --to @foreman --insight "Confirming the connection"
cccc reply LOCAL_EVENT_ID "Received" --group LOCAL_GROUP --insight "Ready to collaborate"
```

Inside a CCCC Actor, `--group` and `--by` default to its injected Group and Actor
identity. Outside an Actor, the active Group and `user` are used. Supply a stable
`--idempotency-key` (MCP: `idempotency_key`) when retrying unchanged content after
an uncertain response; do not create a new message merely because a receipt is
still pending.

Queued means the local instance accepted responsibility to deliver. Sent means
the target Group confirmed receipt; it does not mean an Actor finished the task.
Bounded retries survive restarts. Failed or unconfirmed deliveries produce an
explicit result, and a slow peer does not block healthy peers. Cancelling a
request to reply closes the obligation; it does not retract the message or stop
the receiving Actor.

## Connect a Group with another member

This is a connection between two selected Groups, not a shared administrator
workspace. Both Groups can discover each other's Actors and exchange messages,
replies and files. It does not expose terminals, Presentation, full history,
Context or arbitrary remote tools. Other Groups do not inherit the connection.

1. Both members link their instances to their own accounts and enable a reachable
   HTTPS route in **Settings → Account → Remote Access**.
2. The recipient opens **Group connections** on the account website and copies
   their **Member ID**.
3. The sender opens the intended Group in native CCCC Web as an administrator,
   opens **Group connections** in that Group’s settings or sidebar **⋮** menu, and
   chooses **Invite a member**. On the account website, check the selected Group,
   paste the recipient's Member ID and submit the invitation.
4. The recipient opens **Group connections** on the account website, expands
   **Accept in one of your instances**, and opens their chosen instance. Sign in
   there as an administrator if needed, choose the local Group, then click
   **Review invitation on website**. Check both Groups and confirm.
5. Allow up to two minutes for both online instances to synchronize. Their Actors
   can now use `cccc_connect` to discover the connected Group and its Actors,
   then use the normal message, reply and file tools described above.

An invitation expires after 24 hours. If either instance is offline, reconnect it
and submit the retained confirmation form again before expiry. The selected Groups
and recipient remain visible; retry still checks current ownership and invitation
state. Retrying the same submitted form does not
create another invitation. If a selected Group was deleted or replaced, select it
again in CCCC; if the inviting Group changed, ask its owner for a new invitation.
A temporary local configuration read failure does not revoke an existing
connection: correct the configuration and the same connection can recover.
For a new invitation after cancellation or expiry,
select the Group again in CCCC. A Group can have multiple connections; there is
only one active connection for any exact pair.

Either member can cancel a pending invitation, decline an incoming one, or
**Disconnect** an active connection on the account website. The native dialog
also links directly to the selected connection's disconnect confirmation.
Disconnect takes effect within the authorization lease (at most two minutes);
already delivered messages remain, and running Actors are not stopped. Deleting
or importing a Group, resetting it to a replacement Group, or unlinking its
device requires a fresh connection. Reconnecting never resumes old queued work.

The native **Group connections** dialog distinguishes a confirmed empty list
from pending or failed synchronization. A temporary confirmation failure does not
mean the account was unlinked. The last check and error are shown, and the existing
background service retries automatically. **Refresh** reads its latest result;
it does not create a new connection or restart an Actor.

## Disconnect and historical data

Unlinking a device or withdrawing its account grant ends its Connect authority.
Browser Token revocation ends that browser's access independently; it does not
stop Actors. Offline directory entries are not proof that an instance is online.

[Manual Group Bridge](/guide/group-bridge) is retired. Old grants and unfinished
operations are not silently migrated to Connect. Historical messages remain
readable, with retired remote replies disabled. Remote arbitrary tools and
automatic Web updates are outside this iteration.

## See Group connections

A connection icon and count beside a Group identify its explicit account-managed
and Direct Group connections. Open it to see the peer Groups and manage that
Group's relations. When both methods connect the same pair, it is counted once.
An unconfirmed count shows `?`, not zero. Same-account discovery is not counted
as Group connections. The account website separates pending invitations, active
connections and collapsed history, and offers a return link to your own Group.
Cross-member communication does not grant terminals or a remote Workbench view.

## Direct connections without an account

Choose this when you want two selected Groups to communicate without the CCCC
account service. It shares the same message, reply, file and receipt behavior as
account Group connections. It does not expose the remote workspace, terminals,
history or arbitrary tools. No Web Access Token is exchanged.

Only one instance needs to accept an incoming connection. You can keep both
management Web interfaces on localhost, or manage the connection through the CLI.
The receiving daemon must be reachable from the joining machine, through a local
network, an existing VPN or an explicitly configured network route. CCCC does not
provide a relay, discover devices or change firewall/router settings.

1. On the receiving instance, open the intended Group's **⋮ → Group connections →
   Direct connection** as its administrator. The same panel is in Group settings.
2. Click **Invite another Group**. CCCC selects a real address from the machine
   running the daemon, or retains its saved receiving address. On a shared LAN,
   you normally do not need to type an IP. **Change** offers other local addresses
   (including VPN interfaces) and a custom address for an existing forwarded route.
   Local suggestions are not a test of reachability from the other machine. On
   isolated VM/container/WSL networks, use an address and port the other machine
   can actually reach. The browser's Web URL is not used as a Direct address.
   **Advanced settings** contains the local listen IP/port and instance name.
3. Click **Create invitation**. CCCC enables receiving if necessary, waits briefly
   for the port to start, then creates the invitation. No second confirmation is
   needed after the port becomes ready. Failure shows a corrective next step;
   nothing creates invitations later from background polling. Receiving is shared
   across this instance's Groups, while each Group pair needs its own approval.
   Click **Copy invitation** and send it through a trusted
   channel. Its exact expiry is shown: it is valid for 30 minutes and can pair one
   exact Group. Copy before closing or leaving this panel; the secret cannot be
   retrieved afterward. If you already sent it, wait for the request. If you lost
   the copy, cancel that invitation and create another.
4. On the joining instance, open the intended Group's **Direct connection** panel,
   choose **I have an invitation**, paste it and check the displayed instance,
   Group and expiry, then click **Request connection**. This side does not need
   to enable a listener or open an inbound port. Saving the request is not proof
   of reaching the other machine; the daemon contacts it and waits for approval.
   If the deadline passes before a result arrives, it keeps checking for approval
   granted before the deadline. The panel shows **Checking the inviter’s approval**,
   not confirmed expiry. You can cancel at any time. A confirmed refusal stops
   retries; a previously approved connection can recover after the deadline.
5. Return to the receiving panel. Check the requesting instance and Group,
   verify its identity with the other administrator, then approve it. This side
   explicitly shows that it is waiting for **your** approval. Both panels
   should show **Connected**. Actors can now use `cccc_connect`, then ordinary
   message/file/reply tools with the returned qualified destination.

The first invitation or join uses the current account instance name when
available, otherwise the machine hostname. An explicit Direct name takes
precedence. Peer names and software versions are recorded at pairing time;
keys and Group generations authorize the connection.

Both administrators select their own Group. Pairing and queued messages survive
restarts and closed browsers. Do not copy one running `CCCC_HOME` into a second
simultaneously running instance: that copies its cryptographic identity.

Use **Disconnect** on either side to revoke that Group pair. A revoked grant
cannot be reopened; create a new invitation. Removing a closed record clears its
route preference for future messages but does not redirect already queued work.
An explicitly configured Direct pair never silently falls back to an account
connection when offline. Stopping the listener affects all receiving Direct
connections on that instance, while outgoing connections can continue. The
receiving settings require confirmation before this shared stop. Deleting or
resetting a Group removes its Direct records and frees connection slots, while
keeping retirement IDs to prevent reuse. Closed
invitations and connections remain available in the collapsed history section.

If listening fails, correct the reported bind/port error. If the peer stays
offline, check that the joining machine can reach the advertised address and port,
that the receiving daemon is running, and that neither Group/instance identity
was replaced. A listener being ready does not prove cross-network reachability.
The receiver's listening address and the advertised address can differ when an
existing network route forwards a port. If the local listen port changes, update
its advertised port too unless your forwarding route deliberately uses a different
one. Address changes apply to new invitations; cancel a pending request and create
another invitation if its original address is no longer reachable. Closing the
panel during preparation stops further browser steps, but a confirmed receiving
configuration remains enabled. An uncertain request is read back, never silently
retried; inspect the refreshed status before creating another invitation.

For headless administration, `cccc direct --help` provides the equivalent flow:

```bash
cccc direct listen --bind 0.0.0.0:8847 --address 192.168.1.10:8847 --name Office
cccc direct invite GROUP_A
# On the other machine: paste the invitation on stdin, then end input.
cccc direct join GROUP_B
cccc direct status GROUP_A
cccc direct approve GROUP_A DIRECT_CONNECTION_ID
cccc direct revoke GROUP_A DIRECT_CONNECTION_ID
```

Run each command against the correct instance's `CCCC_HOME`. Invitation import
uses stdin to avoid putting secrets into command arguments or shell history.

If an interrupted deletion left records for a missing Group, administrators can
use `cccc direct status OLD_GROUP_ID`, revoke each connection with
`cccc direct revoke OLD_GROUP_ID CONNECTION_ID`, then remove it with
`cccc direct remove OLD_GROUP_ID CONNECTION_ID`. These cleanup commands do not
recreate the Group or grant access to a different Group.
