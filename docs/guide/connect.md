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

## Disconnect and historical data

Unlinking a device or withdrawing its account grant ends its Connect authority.
Browser Token revocation ends that browser's access independently; it does not
stop Actors. Offline directory entries are not proof that an instance is online.

[Manual Group Bridge](/guide/group-bridge) is retired. Old grants and unfinished
operations are not silently migrated to Connect. Historical messages remain
readable, with retired remote replies disabled. Cross-member sharing of a single
Group, remote arbitrary tools, and automatic Web updates are outside this iteration.
