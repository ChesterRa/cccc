# CCCC Connect

CCCC Connect brings instances linked to the same membership account into one
workspace while each instance keeps its own Groups, files, Actors and history.
Local CCCC use continues to work without an account.

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

Agents use `cccc_connect` to discover instances and their Groups/Actors. Send a
message with `cccc_message_send`, providing both `dst_instance_id` and
`dst_group_id`; use the target's Actor IDs or selectors such as `@foreman`.
`cccc_file(action="send")` supports the same qualified destination for small
attachments. Ordinary `cccc_message_reply` uses the received local Event ID to
return to the original participant, without guessing IDs in another instance.

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

A connection icon and count beside a Group identify its explicit cross-member
connections. Open it to see the peer Groups and manage that Group's relations.
An unconfirmed count shows `?`, not zero. Same-account discovery is not counted
as Group connections. The account website separates pending invitations, active
connections and collapsed history, and offers a return link to your own Group.
Cross-member communication does not grant terminals or a remote Workbench view.
