# CCCC Connect v1

Status: source implementation. This standard defines the same-account
directory, native aggregate Workbench and durable Group/Actor communication,
including MCP discovery/send/file ports, ordinary replies and cancellation.
It also defines account-confirmed connections between two Groups owned by
different members, plus standalone Direct Group connections. Manual Group Bridge is retired. Automatic Web updates and
cross-instance Voice are not implemented.

Linux isolated integration evidence does not claim hosted deployment, public
availability, or native Windows/macOS and other-browser acceptance. Account and
client releases must pass their hosted verification before those claims are made.

Visible managed-service branding uses **CCCC Connect** and **Remote Access**
(the former Reach feature). Existing `reach` provider/CLI/IPC fields, hostnames
and URLs retain their meaning. Local/private/manual Web Access remains available.
Account approval explains same-account background collaboration before binding;
it does not grant cross-instance human administration.

## Authority and identities

The account service owns membership bindings and a directory of public instance
keys. The local daemon owns collaboration, runtime state and persistence. The
account service does not store Group ledgers, messages, attachments, or Web
administrator tokens. There is no Network or same-account per-Group pairing.

An instance uses the existing persisted Ed25519 key and derived peer ID. A device
ID identifies one account binding, not a permanent instance. Restart and migration
of a single Home preserve the key. A missing key file may be created; an unreadable
or corrupt file MUST fail without replacing it. A copied Home must not be run as a
second instance with the same identity. The directory rejects replacement of a
different active binding; it cannot detect two processes holding identical secrets.

Background membership and human Web access are separate. A valid same-account
binding admits background collaboration, independent of Web tokens. The aggregate
Workbench requires administrator access to the entry instance and separate
administrator access to each target. Restricted entry access stays single-instance.
Device credentials MUST NOT be exchanged for Web administration.

## Account directory

`GET /v1/connect/instances` and `POST /v1/connect/instances` require:

- `CCCC-Membership-Version: 1` (account API protocol).
- `CCCC-Client-Version: <actual running CCCC release version>`.
- `Authorization: Bearer <current device credential>`.

The caller must have an active device binding. No account browser session is
accepted in place of that credential. Both methods return the same bounded-lived
directory; GET does not register an identity or consult the tunnel provider.

POST has exactly these fields, bounded to 4 KiB of UTF-8 JSON:

```ts
{
  instance_id: string;
  public_key: string;       // canonical standard base64, 32-byte Ed25519 key
  client_version: string;   // must match the request header
  public_origin: string | null;
  issued_at: string;        // UTC ISO 8601, exactly millisecond precision and Z
  signature: string;        // canonical standard base64, 64-byte Ed25519 signature
}
```

The signature covers UTF-8 JSON of the fixed array, without added whitespace:

```text
["cccc.connect.register.v1", account_origin, device_id, instance_id,
 public_key, client_version, public_origin, issued_at]
```

The account origin is the canonical issuer origin. The proof binds the current
device ID and requires the instance private key as well as the device credential.
Proof time must be within 120 seconds of the account clock. `public_origin` is a
canonical HTTPS origin, without credentials, path, query or fragment; null means
no configured route. HTTP loopback origins are accepted only by a loopback
development issuer with `ALLOW_DEV_LOGIN=1`.

The instance ID uses the existing Ed25519 peer-ID encoding: base58btc of
`[0x00, 0x24, 0x08, 0x01, 0x12, 0x20, ...public_key]`.

Registration atomically checks that the device remains active, one device binds
at most one instance, and one instance binds at most one active device. An existing
active binding cannot be overwritten by another device, even in the same account.
An explicitly retired binding may be replaced after proof of possession. Older
proofs cannot overwrite a newer registration of the same binding. Deleting a
retired device removes its directory record through the database foreign key.

The response is:

```ts
{
  protocol_version: 1;
  account_id: string;
  device_id: string;
  issued_at: string;
  expires_at: string;       // issued_at + 120 seconds
  instances: Array<{
    instance_id: string;
    device_id: string;
    public_key: string;
    client_version: string;
    public_origin: string | null;
    display_name: string;
    registered_at: string;
  }>;
}
```

Entries include only active same-account bindings with an eligible client
version. Registration time is evidence of account contact, not proof that the
remote Web/daemon or tunnel is online. Responses use `Cache-Control: no-store`.
Instance display names use the account device's existing `display_name`; unnamed
devices use `CCCC · <last six label characters>`, never the full routing label.
After a new device grant, the client uses the existing rename endpoint once to
initialize the name from the OS hostname when valid. Failure to save this
optional name does not undo the grant; the name remains editable. Reconnects,
repeated login polling and existing bindings never overwrite an account name.
`POST /v1/device/name { display_name }` authenticates the device and renames only
that active device, with the same validation as the account form (1–60 UTF-16
code units, no control/bidi-format characters); the name must be nonempty. It does
not change instance/device identity, hostname, key, token, or membership.
`connect_rename { by: "user", display_name }` is a serialized user-only mutation,
exposed to Web administrators by `POST /api/v1/connect/name`. Readers keep using
the cached directory. A committed rename is successful even when subsequent
directory refresh fails; the normal refresh loop converges other instances.

The account limits each device to 30 directory requests per minute.

## Daemon ownership, refresh and failure

The daemon owns one refresh task. It starts after daemon initialization, checks
local binding/route intent every 2 seconds, and refreshes an unchanged binding
every 60 seconds. Transient failures back off 5, 10, 20, 40, then 60 seconds.
Account HTTP calls are bounded by the existing membership timeout (15 seconds
by default), occur outside dispatcher permits, and do not depend on an open Web.
Shutdown prevents late results from committing; a changed local binding also
rejects a response from the previous request.

The account-verified snapshot is stored as an owner-only atomic file at
`CCCC_HOME/secrets/connect.json`. Every consumer checks the current local issuer
and device binding, expiry, protocol, self identity and uniqueness of remote IDs.
Readers do not refresh the account or create an instance identity. Temporary
network failure can retain the previous unexpired directory; explicit identity,
version or authorization rejection clears it. Expiry blocks use of the directory
until fresh confirmation. Clock synchronization remains a prerequisite for timed
proofs; a grant is never extended merely because polling failed.

Remote Access intent is preserved. Connect registration neither creates a tunnel
nor turns an explicitly disabled route back on, and does not replace a selected
manual/Tailscale provider. An enabled canonical manual origin can be advertised;
otherwise the directory carries a null route.

`connect_status { by?: "user" }` is a read-only daemon operation returning
`{ connect: ConnectSnapshot | null, account_label?: string | null, group_connections?: { counts: Record<string, number | null>, expires_at: string } | null }`. The snapshot includes issuer/device/instance,
the valid directory or null, `checked_at`, `error_code` and `error_message`.
`GET /api/v1/connect` is its administrator-only Web port. Restricted and anonymous
Web callers do not receive the directory. No private signing key or device/Web
credential is returned.

`account_label` is display metadata from the account's authenticated device and
registration responses. It is stored on the current local membership binding,
not in peer authority or Actor catalogues. Confirmed device or credential rejection
during background refresh clears that binding's label without requiring the Account
settings to be opened. Transient failures retain it; a delayed response cannot clear
a replacement binding. Group counts project only current
cross-member links from the existing expiring grant; they exclude same-account
discovery. A null summary or per-Group count means unconfirmed, never zero.
Summary failure does not invalidate the same-account directory. No additional
per-Group browser poll or account request is required.

Workbench navigation may retain last-known labels after transient read failure
or expiry, but cannot open/retain a frame beyond the existing authorization
lifetime. Explicit rejection, unlink, entry privilege loss, or binding replacement
clears retained navigation. Remote native frames can include the same bounded
count/expiry in their existing Group-list messages; this is display metadata,
not additional access.

## Workbench transport and browser authority

The current workbench candidate embeds the target's native Web document at its
own origin. Group stores, runtime caches and API clients remain target-owned;
the entry receives only instance and admitted Group navigation metadata. The
browser path is still undergoing full integration acceptance.

Each instance needs a distinct hostname for aggregate administrator views.
Different ports on one hostname do not isolate cookies: another port receives
the cookie even if its server ignores that cookie's name. Admission rejects
these overlapping hostnames rather than treating port naming as a credential
boundary. The managed per-device Remote Access hostnames satisfy this condition.
This Web condition does not redefine background instance identity or routing.

HTTPS Web sessions use a host-only `__Host-cccc_access_<port>` cookie, with
`HttpOnly; Secure; SameSite=None; Partitioned; Path=/`. HTTP loopback sessions use
`cccc_access_<port>` with `HttpOnly; SameSite=Lax; Path=/`. Cookies expire after
30 days; the target still validates the underlying current token on each request.
The previous unqualified cookie is no longer consumed, so existing remote Web
sessions require a one-time login after upgrading. Browser partitions are keyed
by entry site: login reuse is within the same entry-site/target pair, not a
promise that a standalone target login unlocks every other entry site. No token
is copied between those sites or sent to the account service.

Before presenting a target login, the entry performs a server-side target
identity challenge over its advertised route. Redirects are forbidden, the
request has no credentials, the deadline is 5 seconds, and the response is at
most 4 KiB. `GET /api/v1/connect/identity?nonce=<UUID>` returns a 30-second signed
proof containing instance ID, current device ID, public origin, actual client
version and the challenge. The signature covers the fixed array defined by
`ConnectIdentityProof::signing_material` in `cccc-contracts`.

`POST /api/v1/connect/open { instance_id, frame_id }` requires current entry
administrator access, including a recheck after the network challenge. It returns
a target `/ui/connect/` URL and an Ed25519 frame proof binding account issuer,
both instance/device IDs, exact parent origin, frame ID, nonce, issue and expiry
times. `ConnectFrameProof::signing_material` defines the ordered signing array.
Proof lifetime is at most 120 seconds, with up to 30 seconds of future clock skew.
This proof permits embedding only; it does not create a target Web login.

The target verifies current same-account membership and stores at most 128 live
frame proofs in its Web process. Only the admitted page receives the exact parent
in `frame-ancestors`; ordinary pages retain same-origin embedding policy. An
already opened frame cannot be opened again with that proof. Public signed
`POST /api/v1/connect/frame` renews an existing frame with a newer issue time and
fresh nonce without changing its origin or bindings. Replayed renewals fail.
The ephemeral registry is not a durable replay store: a Web restart ends live
frames; an old proof still within its original expiry may open again after a
restart, but never creates a human login or extends its authority.

Administrator `GET /api/v1/connect/frame?frame_id=...` checks current target
authority. Nested target-owned Presentation resources carry only `connect_frame`
to select that existing embedding authority; they still require the target
token. Target downloads use authenticated fetch and a Blob URL because native
download navigation can leave the embedded cookie partition. No bearer token
is placed in a resource URL or forwarded through the entry.

All parent/child messages check exact origin, window and frame ID. Each entry
navigation carries a monotonically increasing revision; target selection reports
echo it. The entry ignores reports from earlier revisions, including a late
initial/default Group selection, without remounting the frame. Target-local
navigation may update the entry within the current revision. A `select` message MAY carry
`action: "connections"` to open the Group connections dialog for that Group after target
administrator admission and Group existence checks. This is applied at most once per
navigation revision, performs no sharing mutation, and does not grant additional access.
Ordinary navigation clears the previous Group dialog. Restricted
tokens and unconfigured target bootstrapping never mount the embedded workbench
or disclose its Groups. The entry and active target check current human access
every 15 seconds; frame proofs renew every 45 seconds and expire without fresh
authority. Only the selected remote workbench owns full event/TUI connections;
leaving it releases those connections. Previously viewed Group navigation remains
in entry-page memory, independently of the selected workspace and explicit
expand/collapse state. Inactive lists are labeled as saved navigation, not current
online or administrator confirmation. Reopening always authenticates the target
again before receiving content. Expired entry directory/entry access, device or
origin replacement, and an active target lock clear the affected navigation.
No inactive iframe, TUI connection, bearer cache, or content synchronization is
kept alive merely to preserve the sidebar. Realtime event and terminal connections also recheck current token access
every 15 seconds; embedded resource connections additionally check the frame.
Embedded Group resources, the shared `/api/v1/events/ws` socket and the legacy global `/api/v1/events/stream` carry
`connect_frame`; WebSocket URL construction compares `ws`/`wss` with the
corresponding `http`/`https` origin, preserving the exact host and effective port.
The shared socket and global stream check current frame and target administrator authority before
emitting metadata and on the same 15-second interval. An expired frame or either
retired device binding closes embedded streams even while the target Token
remains valid and the browser has not unmounted the workbench. Ordinary
single-instance streams retain their Token-based Group filtering.
Revocation closes browser connections, not the Actor or accepted daemon work.

The directory tests and the Chrome cookie/SSE/WebSocket primitive probe are not
evidence of complete browser/platform support. Full native-workbench, host/path,
revocation and resource regression results are recorded separately. Cross-instance
Voice remains a later integration; this iteration does not expand microphone
delegation policy across origins.

## Versions and rollout

CCCC 0.4.40 is the first client version implementing this contract and the
initial minimum for Connect registration/discovery. Product versions are
distinct from the account and Connect protocol numbers. A prepared version is
not evidence that its artifacts have been published.

The account environment `MEMBERSHIP_MIN_CLIENT_VERSION` is the single deployment
minimum for daemon membership business routes. When set, missing or older product
versions receive HTTP 426 with the existing `unsupported_version` error shape.
Connect registration/discovery always require a reported eligible version. Their
effective minimum is the higher of 0.4.40 and the common deployment minimum; a
lower membership minimum does not admit older Connect peers. Until
the deployment minimum is enabled, existing device/login/Reach routes retain their
previous compatibility so the reporting client can be distributed first. Account
browser routes and device retirement are not blocked by the daemon product gate.

The gate does not delete bindings, retire hostnames or claim that an existing
tunnel has stopped. Raise the hosted minimum only after the new client artifacts
and upgrade instructions are available. Native update/remote recovery and bounded
revocation of complete peer/Web sessions remain integration acceptance work, not
evidence supplied by directory tests alone.

Account rollout order: apply additive migration `0010_connect_instances.sql`,
deploy the compatible Worker, distribute the reporting client, then deliberately
enable the common minimum. Previous Worker code can operate with the added table.

## Peer transport and discovery

The daemon uses one direct HTTP peer path. It verifies the actual target with the
same signed identity challenge as the Workbench before sending a peer operation.
Redirects are rejected. This does not add a cloud business-message relay or copy
legacy per-Group pairing/session records. Both peer bindings come from the current
account directory; each network operation has a 5-second timeout.

`POST /api/v1/connect/peer` carries the typed `ConnectPeerOperation` as JSON.
`CCCC-Connect-Proof` contains base64url (without padding) JSON of
`ConnectPeerAuthorization`, bounded to 4 KiB. The proof signs issuer/account,
source and target instance IDs and device generations, UUID request ID, issue and
expiry times, and SHA-256 of the serialized typed operation. The ordered array is
defined by `ConnectPeerAuthorization::signing_material`. Thus metadata and blob
content are bound to the same proof. Proof lifetime is at most 60 seconds with
30 seconds of future clock skew. Unknown operations and fields are rejected.

The Web parts extractor verifies the current device proof before reading a body;
anonymous uploads are rejected without consuming their attachment stream. The
body limit is 14 MiB, allowing at most 10 MiB of base64-encoded file content plus
bounded message metadata. Exhibit listeners reject the operation. Web forwards
`{ group_id: operation.target_group_id(), envelope: { proof, operation } }` to
`connect_peer_receive`; the daemon independently verifies current authority,
operation digest and exact dispatcher Group. Deliver and cancel use target Group Write;
catalog/receipt use Read. No human Token or arbitrary MCP operation is accepted.

The typed operations are catalog, deliver, receipt and cancel. Receivers sign the result
against the request ID and SHA-256 of its signing material; the sender verifies
the response signature, request binding and current membership before use.
Business errors are signed too. HTTP success alone is not delivery confirmation.

`catalog { source_group_id, target_group_id?, after? }` permits an empty source
Group for daemon-level same-account discovery. A Group-pair authority must
supply its exact nonempty source and target; it cannot discover other Groups or
inherit the target's same-account authority. Catalog, deliver, receipt and cancellation consumers
have isolated Group-pair negative tests. External invitation and member approval
use the account-managed Group connections described below.

Catalog pages contain at most 64 Groups, ordered by Group ID, with a `next` cursor.
They expose Group ID/title and visible Actor ID/title/enabled/role/generation only,
not paths, commands/environment, histories, Context or TUI. A page over 1 MiB fails.
Catalog discovery scans valid directory entries every 2 seconds with at most 8
peers in flight. Success refreshes after 60 seconds; failures back off
5/10/20/40/60 seconds. A refresh is bounded to 30 seconds, 16 pages and 4 MiB of
normalized metadata. Partial catalogs are not published as complete.

`CCCC_HOME/state/connect/catalog/<instance_id>.json` is an atomic owner-only cache,
bound to issuer/account, local device, remote instance/device/origin and success
time. Reads and writes validate the current directory. Metadata is fresh for
120 seconds; older metadata remains explicitly stale so a known offline recipient
can still be identified by its fixed Actor generation. This never extends
membership authority or proves reachability. Binding changes reject late results
and old caches. `connect_catalog { group_id, instance_id?, by?,
target_group_id?, after?, limit? }` checks local Group membership. Without
`instance_id`, it returns the current remote instances, local instance ID and
qualification state/expiry. Without membership, the instance-wide directory is
empty; approved Direct Group pairs remain available in `external_groups`.
An unreadable optional account cache reports `account_error` while preserving
independently authorized Direct discovery. It never supplies an account grant.
With `instance_id`, it returns `{ instance, catalog, fresh, next }`, filtering
optional exact target Group and cursor. The local default page size is 20,
clamped to 1–64. It never starts a refresh or Actor. The core MCP tool
`cccc_connect` exposes this read-only directory to every local Actor role.

Group-scoped connections require both `instance_id` and `target_group_id` for
detail reads. If a caller supplies only the instance and has authorized external
Groups there, the operation returns `connect_target_group_required` with those
choices in `error.details.external_groups`; it MUST NOT silently select a Group
or broaden access to that instance's catalog. Supplying a target Group without
an instance returns `connect_instance_required`. Error choices are restricted to
the calling local Group under the same authorization as normal discovery.

The Web composer reads the same cache through administrator-only
`GET /api/v1/groups/{group_id}/connect/catalog`, with optional `instance_id`,
`target_group_id`, `after` and `limit` query fields. The path fixes the local
source Group and the port fixes `by=user`; exhibit access is denied. Embedded
requests carry the ordinary Group resource's frame authorization. This read
does not refresh peers or require a target Web Token. Restricted Web sessions
retain local-only mention discovery, independently of background Agent grants.

Selecting a remote `#Group` creates a `connect_group_ref` in the local message's
`refs`; it never changes `to` or sends remotely. Candidates distinguish instances
and include same-account catalogs and exact externally connected Groups (account
sharing or Direct). Cached metadata can be marked stale; it is not online proof.
The selected identity is retained with the local Group's draft. Plain pasted
text does not create a reference. An Agent rechecks the current connection before
acting; a draft or historical reference cannot keep a revoked grant alive.

The CLI `cccc connect` is a thin `connect_catalog` port. `cccc send --dst-instance
... --dst-group ...` uses the same qualified route and retry-key semantics as MCP;
`cccc reply` uses the local received Event ID and omits recipient overrides by
default. CLI messaging accepts `--insight` and `--idempotency-key`. Remote terminal
envelopes identify the source names and qualified IDs, and point replies at the
local Event ID rather than a source instance's Event ID.

## Durable message transport

The daemon-owned `connect_send` operation accepts `group_id`, target `instance_id`
and `target_group_id`, `client_id` (nonempty, at most 128 bytes), `by` (default user),
`text`, `message_mode`, `to` (default foreman), optional format/insight and existing
source Group blob attachments. It requires a local user or visible Group member;
an incoming external sender is not a local member and gains no transitive send
permission. MCP `cccc_message_send` maps `dst_instance_id` plus `dst_group_id`
to this qualified route, even when a remote Group ID equals the source Group ID.
It maps `idempotency_key` to `client_id`, generating a key if omitted. An explicit
malformed or incomplete remote address fails instead of falling back locally.
The operation takes only the source Group write permit and performs no network I/O.

Recipients use the existing recipient normalization and Inbox audience rules.
Aliases are materialized against the known target catalog into concrete Actor IDs
and generations, including for ordinary sends. Actor creation/recreation uses a
fresh UUID generation; editing and restarting preserve it. Existing actors use
`legacy:<created_at>` until recreated. A delayed send must not retarget a new actor
with the same ID. Group/Actor title changes do not change their identity.

`ConnectMessage` in `cccc-contracts` is the immutable logical record: delivery ID,
issuer/account, both Group addresses qualified by instance and device, source
sender generation, preallocated source event ID, concrete recipients, text,
format/insight/mode, attachment descriptors, reply reference and business times.
The message allows at most 64 KiB text, 128 recipients and 16 attachments totaling
10 MiB. Creation permits 30 seconds future skew. Delivery lasts at most 15 minutes;
reply eligibility lasts at most 7 days. Those business deadlines do not extend the
short-lived wire proof. Replies cannot create request_reply obligations.

`connect_send_files` shares local file canonicalization and blob storage with
ordinary `send_files`. It accepts 1–16 files under the active source project
scope and reads at most the 10 MiB total limit plus one detection byte. Symlink
escape and files outside that scope fail. Its retry lookup precedes reading the
original paths, so a deleted original file cannot break an already accepted retry.
MCP `cccc_file(action=send, dst_instance_id, dst_group_id, idempotency_key?)`
uses this daemon operation, reporting durable acceptance and queue status. It
exposes no remote file-system tool. Local resource refs and composer suggestions
are not shared; only message text and copied attachments cross instances.

Acceptance persists the logical message, the complete preallocated source event
and mutable progress in an atomic owner-only active outbox file at
`state/connect/outbox/<peer>/<delivery_id>.json`. It is capped at 1,024 active
records, 128 per peer, and referenced file bytes of 256 MiB total / 64 MiB per peer.
A single record is capped at 512 KiB. Payloads remain in existing source Group
blobs; base64 is transient wire data. Unreadable active records consume the full
per-message file budget, remain available for recovery, and do not by themselves
prevent admission of healthy messages. Queue exhaustion rejects new work explicitly.
A stable caller key cannot change an accepted body or target. Only committed
acceptance returns `accepted:true`; this is not a claim of remote delivery.
Responses include `queued` and `delivery_state`. A retry after terminal cleanup
returns the original source event and receipt state; a missing active record and
missing receipt is explicitly unconfirmed, never an invitation to silently resend.

The source ledger projection uses the preallocated event and original caller
`client_id`, so browser outbox reconciliation retains its normal identity.
If that projection fails after outbox acceptance, the response preserves acceptance
and the background worker recovers the same source event. No second completed
receipt database is introduced. Mutable progress records whether a POST may have
escaped, retry timing and errors; it cannot alter the logical delivery identity.

`deliver { message, blobs }` verifies current binding, exact resource scope,
logical limits and original recipient generations. It looks for an earlier ledger
receipt before rechecking delivery expiry, current actor generation or files, so
an already committed delivery remains recoverable after those states change.
Conflicting content under the same delivery ID fails. Each transient blob carries
its SHA-256 and base64 bytes; the complete attachment set, sizes and hashes are
validated before any target blob or message is written. The final event uses the
ordinary message/Inbox delivery mechanism, without creating or starting actors.

The received canonical event retains source instance/Group/event and sender
provenance plus `connect_message` and its digest. Local source/destination
projections may carry `src_instance_name` / `dst_instance_name` snapshots from
the authenticated account directory. These optional display names do not affect
wire identity, authorization or idempotency; historical events need no rewrite.
An untitled remote Actor is displayed by its actual Actor ID. Its sender is an external
`connect:<source_instance>` identity, never a local actor impersonation. A receipt
contains the logical delivery ID/digest, target event ID and commit time. It is
reconstructed from the existing ledger idempotency index, including archived
segments; a process crash after append does not require a new incoming record.

`receipt { source_group_id, target_group_id, delivery_id, message_sha256 }` returns
that original receipt or null. It rechecks the same source/target resource and
binding. A deleted target Group returns an error, not a claim that an uncertain
past delivery was absent. When a POST outcome is uncertain, the sender must query this receipt
before resending. A missing signed receipt allows a retry of the same immutable
ID/body; a timeout or unsigned response does not. Received reply references also
check the original Group pair, sender/recipient generations and reply deadline.
The ordinary `reply` operation derives the peer, Group, original participants
and target event from the local canonical event. A reverse reply requires no
reverse catalog: it uses the original sender snapshot. A forward follow-up from
an outgoing source record requires a positive original delivery receipt. The
original sender may follow up to a subset of the original recipients; only an
original recipient may answer as that Actor, with the same generation. A local
Group human may intervene as `user`, never impersonating an Actor or fulfilling
that Actor's obligation. Aliases cannot expand a reply audience. Title changes
do not change identity; Group/device/Actor replacement or expiry cannot retarget
an existing reply. `message_upload_preflight` uses the same routing/participant
checks before uploads and does not enqueue work or project events.

Reply request status on the source uses the remote instance, Actor ID and
generation; a colliding local Actor ID cannot fulfill it. Live Web projections
resolve the reply against the original message's canonical Connect participants,
Group/device addresses and reply reference before updating that recipient's
status, both in the live tail and an open history window. They MUST NOT treat
`event.by = connect:<instance>` as a local Actor ID or infer identity from titles.
The destination's
ordinary local reply projection fulfills its local obligation.

`reply_request_cancel` derives a Connect control from the original local event.
The original sender must retain the same Actor generation; a human in either
participating Group may cancel as `user`. Recipient Actors cannot cancel a
sender's request. The operation withdraws the reply obligation, not the message
or an Actor task. It neither stops a runtime nor automatically starts one.

`ConnectWork::Message | Cancel` shares one active outbox, resource limits,
scheduler, final receipts and recovery. A cancellation is reserved before the
local `chat.reply_request.cancelled` projection, with a stable ID for that
original delivery and issuing Group. Its immutable record binds the original
message ID/digest/source event/deadline, account and both qualified Groups.
`cancel { cancellation }` verifies that relationship against the receiver's
original ledger message. It never expands to another Group or current Actor
with a reused ID. Received controls are recorded locally, not forwarded again.

A cancellation that arrives before a still-deliverable original returns signed
`connect_original_pending`: only that job waits five seconds. The peer remains
eligible to deliver the original from the same queue. If the original is still
absent after its delivery window, the receiver records an idempotent no-op
cancellation audit (`source_event_id:null`, `not_delivered:true`); there is no
obligation to retract and the expired original cannot later be newly admitted.
An accepted cancellation is recovered from the canonical/archived ledger before
expiry checks. Changed content under its ID is rejected. Cancellation retries
replay the identical small control, with a fifteen-minute delivery window; they
do not need a second receipt-query protocol. Uncertain expiry remains unconfirmed.

The local cancellation takes effect on acceptance. Its `connect_cancellation`
status separately reports queued/sent/failed/unconfirmed propagation, for either
direction. A receipt has `action:"cancel"`, `source_event_id` identifying the
control and `original_event_id` identifying the local original message. It must
not overwrite that message's original delivery status or remote reply anchor.
Queries with `with_obligation_status` annotate `_connect_cancellation`; Web live
projection and history use the same states. Cancelled and replied are displayed
separately. Confirmation means both Groups recorded cancellation, never that
an already running task stopped.

A separate bounded delivery scheduler shares the same HTTP client and protocol
with discovery. It scans active IDs once per second and when a worker finishes,
so ready work can use released capacity without waiting another poll interval.
It runs at most 4 jobs and allows only one active job per peer. Available slots
rotate among eligible peers so busy queues cannot starve another destination.
Retries persist and back off 5–60 seconds;
local corrupt records or projection failures defer only that job, while network
failures defer the unavailable peer. Other peers and healthy records remain
processable. Dispatcher permits cover only local projections; acquisition itself
is bounded to 5 seconds, and no HTTP wait retains a permit. Service shutdown
cancels its owned tasks, leaving their durable active records for recovery.

The delivery window ends with sent, failed or unconfirmed status; uncertain work
does not block the queue forever or become an implicit new send. A final
`chat.cross_group_receipt` is appended to the source ledger before removing active
work. Failed/unconfirmed work also creates an idempotent internal `system.notify`
for the original sender, or the Group user if that Actor generation no longer
exists. This uses the ordinary Inbox without starting an Actor. Failure to persist
that notification retains active work; restart completes notification and cleanup
from the final receipt without another POST. Explicit source Group deletion also retires its remaining active records,
without recreating the deleted ledger. An already in-flight request cannot be
retracted by deleting the source Group. Ordinary local disk failures remain
visible recovery errors, not silent message deletion.

The source event includes destination instance/Group/title and recipient title
snapshots. The received event includes `source_platform=cccc_connect`, original
Actor ID/name and source instance/Group/title. Consumers use those snapshots,
not a same-ID local Group or Actor. Web replies keep the local event/Group anchor;
the daemon derives the qualified route. Source navigation must not open an
unrelated local Group with the same ID.

`ledger_statuses` includes `connect_delivery { state, error?, remote_event_id? }`
for outgoing Connect messages. `state` is queued, sent, failed or unconfirmed;
final state comes from the canonical receipt. Queries with
`with_obligation_status=true` also annotate `_connect_delivery` on events.
The Web projects live receipts immediately and hydrates the same state through
normal ledger status reads after reconnect/history loading. A late pre-receipt
snapshot cannot downgrade a known terminal state to queued. This annotation is a
read projection, not a mutation of the immutable source event. Delivered means
received by the Group, not read, answered or executed by an Actor.

These mechanics have isolated HTTP/fault-injection evidence, including lost
responses, archived message/control receipts, recreated recipients,
duplicate/conflicting IDs, source projection and failure-notification recovery,
corrupt record isolation, cancellation deadlines and source deletion. Three real
fixture daemons and native Web routers additionally verify B/C messaging,
restart recovery, replies and cancellation while entry A is offline, followed by
account retirement. Expanded isolated Chrome checks cover target logout/reopen,
keyboard settings and narrow-viewport navigation alongside Token separation,
TUI, files and revocation. These tests use temporary Homes and no paid Providers;
hosted/native-platform acceptance is a separate release gate.

## Retiring manual Group Bridge

Manual pairing, registered message/read/full grants, session WebSockets and
`cccc_remote_*` tools are removed. These grants never become account grants.
Ordinary local cross-group messaging remains. The daemon startup and historical
receipt rules are specified in [Daemon IPC §8.17.2](CCCC_DAEMON_IPC_V1.md#8172-cccc-connect-and-manual-bridge-retirement).

The Ed25519 instance key retains its persisted `group_bridge_identity_key.yaml`
filename and bytes. Only the code API is renamed to `InstanceIdentity`; generating
a replacement key merely because the feature name changed is forbidden.
Same-account collaboration uses current account/device identity independently
of human Web Tokens. Sharing a Group with another member uses a separate,
explicit Group-pair grant, never reactivation of manual Bridge.

## External Group connections

This extension is separate from the same-account directory and browser Workbench.
An active connection binds two different account IDs, instance keys, current device
IDs, Group IDs and their resource generations. New/imported Groups receive a fresh
`generation`; pre-existing Groups without it use `legacy:<created_at>` until an
import replaces their generation. Rename preserves identity;
delete/reset/recreation invalidates it. Only the two selected Groups can discover
each other and exchange explicit messages, replies, cancellations and transferred
attachments. This grant is not transitive and never grants browser administration,
full ledger/context reads, filesystem access, terminals or Presentation.

The local administrator selects a Group and obtains a signed selection ticket.
The account browser requires the matching member session plus CSRF protection to
invite a specific member ID. The recipient selects their own Group and explicitly
accepts. Tickets expire after 24 hours. A ticket signature plus recipient determines one
invitation ID; replaying the same form is idempotent and cannot reopen a closed
invitation. A fresh selection is required to invite again. Creation and acceptance validate the
selected resources through their registered HTTPS endpoints; offline Groups fail
clearly and may be retried. No device credential can approve membership sharing.
The account stores only this relation and minimum endpoint metadata. Instance
and Group names in invitations are selection-time labels; IDs and generations
define authority, and renaming does not revoke a connection.

The signed ticket material is the fixed JSON array:
`["cccc.connect.group.ticket.v1", account_origin, account_id, instance_id, instance_name, device_id,
group_id, group_generation, title, issued_at, expires_at]`.
`POST /api/v1/connect/group-check {ticket,nonce}` proves that this exact Group still
exists at its registered instance. Its signed response covers
`["cccc.connect.group.check.v1", ticket_sha256, nonce, title, expires_at]`, with a
30-second lifetime; the digest covers ticket signing material. This endpoint only
checks the selected identity, not a catalogue or any work data. Signature and
validity checks precede resource reads. HTTP 403 `connect_group_denied` means an
invalid selection (including a missing or replaced Group); the member must select
again, or request a fresh invitation if the source Group changed. HTTP 503
`connect_group_unavailable` means a temporary read/confirmation failure; the same
selection may recover. The account consumer uses the native error code, not a
bare HTTP 403 from an intermediary, to choose the terminal recovery path.

`GET /v1/connect/groups` uses the existing version and device authentication and
returns only active connections involving that device. Authorizations last at most
120 seconds and are refreshed by the existing daemon service. POST on the same
endpoint may invalidate only connections involving the authenticated device when
its selected Group is confirmed missing or has another resource generation.
Read/parse errors deny access but do not prove permanent retirement: they remain
visible synchronization errors while healthy Group grants keep renewing. Recovery
of the same resource preserves the original connection. Readers never initiate refresh work. Device invalidation accepts at most 128 UUIDs
in an 8 KiB request; registration and resource proofs retain their 4 KiB limits.
Failure to deploy this optional endpoint does not disable same-account Connect.
The existing local Connect snapshot records `group_sync` diagnostics separately
from account-directory errors; it grants no authority. Failure retains only still
valid grants and successful synchronization clears the sharing error.

Each accepted connection has a unique immutable ID. Logical deliveries and their
transport proofs pin that ID; a newly accepted connection cannot revive old queued
messages, receipts or reply capabilities. Either member may disconnect; no new
operation is admitted after local revocation/lease expiry. Already delivered copies
remain, and running tasks are not stopped. Revocation propagation is bounded by
the existing 120-second issuer lease. Concurrent accepts/reverse invitations must
converge to at most one active connection for the same exact endpoint pair.

### External connection wire scope

`ConnectPeerAuthorization`, `ConnectMessage`, `ConnectCancellation`, and the
`Catalog`/`Receipt` operations have an optional `connection_id`. When absent it is
omitted from serialized JSON, retaining the same-account signing material. With
an external ID, the request signing domain is `cccc.connect.peer.group.request.v1`,
with the connection ID appended to the existing request array. The signed request
and logical operation must carry the same ID. `account_id` in these external peer
requests/messages identifies the connection's initiating account (source endpoint
in the cloud record), consistently in either direction; the current local account
is independently verified against the issuer lease. It does not grant access to
other resources owned by the initiating account.

Both endpoint device IDs, local Group resource generation and the exact pair are
checked before signing and receiving. Responses continue to bind the full request
hash. Receipts and replies must match the original connection ID as well as its
participant generations. Connection-scoped catalogues are stored independently
from instance-wide navigation; they contain only the selected target Group.
Successful issuer refresh removes catalogues for retired connection IDs; it never
deletes delivered messages or attachments.
A fresh issuer snapshot missing a queued connection is authoritative revocation:
unsent work becomes failed; work awaiting a receipt becomes unconfirmed. Loss of
the issuer is unavailability, never proof that an uncertain delivery did not occur.

### Management ports and user journey

- `connect_group_status {by:"user",group_id}` is read-only and returns current
  active links for this Group, expiry, account origin and current account ID.
  `status` is `not_linked`, `syncing`, `ready` or `unavailable`; it includes
  `checked_at`, `error_code` and `error_message` for sharing synchronization.
  Only `ready` with an unexpired empty grant confirms no active links. Missing or
  expired confirmation must not be displayed as no connections or an unlinked
  account. Linking is determined from current device membership, not directory freshness.
- `connect_group_select {by:"user",group_id,invitation?}` performs no sharing
  mutation; it signs this existing Group selection and returns the account
  confirmation URL. Web uses explicit POST to initiate this selection.
- `/api/v1/connect/groups` exposes these operations only to Web administrators
  (GET status, POST selection); restricted views/exhibits cannot manage links.
- The account `/connect` page shows the member ID, pending invitations, active connections and collapsed history.
  `/connect/select` requires an authenticated member matching the selection ticket.
  GET presents confirmation; CSRF-protected POST creates or accepts an invitation.
  Transient resource-unavailable errors retain the selected Groups and recipient
  on the confirmation form. Retrying requires another explicit submit and rechecks
  session, CSRF, ticket validity, invitation state and both live resources.
  A definitive native selection rejection instead explains reselection/new invitation
  and does not offer repeated submission of the unusable ticket.
  Recipient links open `/?connect_invite=<id>` on a selected owned instance; the
  native Group chooser does not automatically accept an invitation. Cross-site
  links preserve supported `lang` values. After browser confirmation, the account
  page identifies the affected relation and offers a `?group=<id>&lang=<lang>`
  navigation link only through the member's currently registered matching device
  and instance. This return link does not bypass native Web authentication or
  confer remote Group access.
- Cancel/reject/revoke use CSRF-protected browser POSTs. Disconnect also has a
  confirmation page. Device retirement closes its invitations and connections
  atomically; account deletion removes its relation metadata. Closed records and
  expired invitations are retained for up to 30 days before routine cleanup.
- Account management limits each member to 20 mutations/minute and 128 outstanding
  originating invitations/connections. Acceptance limits each member to 128 active
  connections. Device synchronization is bounded to 30 requests/minute.

For `cccc_connect`, omitting `instance_id` returns same-account `instances` plus
`external_groups` for the calling Group only. Each external entry includes
`connection_id`, public instance metadata, `group_id`, and `title`. To discover
that Group's Actors, supply both its `instance_id` and `target_group_id`. Sends
continue to use `dst_instance_id` and `dst_group_id`; the daemon resolves the exact
authorized connection. Ordinary replies and file delivery reuse existing ports.

## Standalone Direct Group connections

A Direct connection grants collaboration between exactly two Group generations.
It needs no account, directory lease, public domain, Web listener or Web Token.
It uses the current catalog, messages, replies, files, cancellation, durable
outbox and signed receipt implementation. It grants no terminal, arbitrary tool,
filesystem/history or aggregate Workbench access. Multiple pairs are independent;
a connected Group does not relay or discover its other connections automatically.

The daemon owns a separately enabled TCP listener, with an explicit listen IP and
port and an advertised `host:port`. Only the joining instance needs an outbound
route to that address. The single socket carries requests and replies from both
sides; no reverse inbound port is needed. Web Access, browser sessions and
membership remain independent. Disabling the listener stops receiving channels;
using invitations to connect outward remains possible. This setting applies to
all receiving Direct connections in the instance. No NAT traversal, relay,
broadcast discovery, firewall edits or router configuration is performed.

Transport uses TLS 1.3 with mutual Ed25519 raw public keys (RFC 7250) and ALPN
`cccc-direct/1`, using the persisted instance key. The joining client pins the
invitation's exact host key. The listener verifies possession of the presented
client key, then independently checks its invitation and approved Group grant.
Possessing a TLS identity alone grants no application access. Ordinary HTTP
account-peer endpoints MUST reject Direct authority; ordinary HTTPS certificate
validation is unchanged. A copied Home is the same cryptographic identity and
must not be run as an independent instance.

An invitation is `cccc-direct:` plus base64url JSON with version `v:1`, a fresh
`direct-<UUID>` ID, address, exact host instance/key/Group generation, expiry and
random secret. The 30-minute invitation is shared through a trusted channel;
it is not a navigation URL. The receiving store holds a secret digest. The
joining store retains the invitation privately for authenticated reconnects.
An invitation can bind one requesting peer and one exact Group. Import is
idempotent for that pairing. The receiver must explicitly approve the request;
joining does not self-approve. Pending state survives restarts. Expiry blocks
new approval, not an already approved connection. A locally pending join MUST
continue to reconcile the pinned receiver's decision after its deadline, with a
slower retry cadence (60 seconds after a failed attempt, rather than 5). Network
failure leaves the result unknown. Only an authenticated receiver can confirm
`active`, terminal `expired` (no approval before the deadline), or `revoked`
(including a removed/unknown invitation). Terminal decisions stop dialing. No
expired invitation can be newly imported or approved; this recovery only checks
requests already persisted on the joining side. Revocation cannot be undone;
reconnection requires a new invitation. A Group replacement or identity change
invalidates its grants. Names are descriptive snapshots, not authority.

`direct_connections.json` is owner-only daemon state. Removed grant IDs remain
as compact retirement markers, so removing a row cannot make an old invitation
usable again. Their secrets, names and disposable catalogs are removed. Successful
Group deletion and reset retire the deleted Group's records and free their quota.
A deletion rolled back before unregistering preserves its Direct records. An
administrator can inspect, revoke and remove orphaned records by their original
Group ID if a prior deletion was interrupted; no new grant or approval is allowed
for a missing Group. GET/status calls neither
create identities nor open listeners or start jobs. Status omits invitation
secrets and distinguishes configured, listening, pending approval, active but
offline, online, expired and revoked. A joining `pending` record past its deadline
stays visible as checking approval, until confirmed or explicitly cancelled; the
local clock alone does not place it in closed history. Online is bounded-fresh channel evidence,
not proof that a particular message was received. Listener errors and connection
attempt errors are shown separately. Polling does not reset form drafts.

Direct uses the existing typed peer operations. Its `connection_id` is mandatory
and uniquely names the local grant; account/device fields are absent on the wire
and represented as empty internally. They MUST NOT contain fictitious accounts
or cloud lease values. Signed request material uses the existing ordered fields
and connection ID with domain `cccc.connect.peer.direct.request.v1`. Each request
is checked against the current key, exact pair, resource generation, digest and
60-second request lifetime; responses remain signed and correlated to that
request. A live channel is not continuing authorization. The dispatcher uses the
same Group locks as other ports; network waits hold no dispatcher permits.

Frames are a four-byte big-endian length followed by JSON, bounded to 14 MiB;
the initial hello is bounded to 8 KiB. Handshakes and writes have 10-second
budgets, idle reads 45 seconds, with idle keepalives. Queues and request
concurrency are bounded. Connection loss returns uncertainty to the existing
outbox; no second durable queue is introduced. Receipt recovery precedes retry,
so losing a response cannot create a new logical delivery. Malformed or stalled
peers cannot hold a global dispatcher lock.

An explicit Direct record pins the route for its exact pair even while offline
or revoked. It never automatically falls back to account authority. Removal is
allowed only after revocation or confirmed unapproved invitation expiry (the
receiving side owns its decision; a pending joining side must reconcile or cancel).
Removing a pin
permits future independently authorized routing, but existing work remains bound
to its original connection ID. A known revoked/deleted/replaced grant retires
pending work under the existing failed versus unconfirmed receipt rules.

Administration uses `connect_direct_status`, `connect_direct_configure`,
`connect_direct_invite`, `connect_direct_join`, `connect_direct_approve`,
`connect_direct_revoke`, and `connect_direct_remove`. These are user-only daemon
operations. Web exposes administrator-only GET/POST `/api/v1/connect/direct`;
restricted, anonymous remote and exhibit sessions cannot manage them. Actor MCP
continues using `cccc_connect` discovery and the existing messaging tools;
it cannot create or approve grants. The CLI exposes `cccc direct`; invitation
import reads stdin rather than accepting a secret in the command line.
