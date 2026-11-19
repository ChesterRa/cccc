# CCCC private session ownership contract v1

This contract separates managed member execution from the collaboration daemon.
It is private to CCCC and versioned independently of product versions and the
public daemon IPC. Linux and Apple Silicon Macs running the native arm64 build
use an independent session owner. Intel macOS builds and Windows use in-process
runtime ownership and reject maintenance preservation as unsupported.

## Ownership and authority

The daemon remains the sole collaboration scheduler and ledger writer. A
session owner holds the existing managed-session objects, PTYs and DeepSeek
execution loops. It executes only explicit finite commands. It does not select
Inbox work, render new tasks, append collaboration events or restore members
from saved Group configuration on its own.

An owner establishes an independent OS session before creating member
resources. Its process guard has a separate ownership ledger; replacing or
cleaning up the daemon must not reclaim the owner's process tree. A normal
daemon shutdown explicitly stops retained owners. Loss of a control connection
alone does not stop admitted execution. Maintenance exit releases control
without waiting for active provider turns.

The discovery directory is `${CCCC_HOME}/daemon/session-owners/<owner UUID>/`.
It contains the owner lock, address, control socket and process-ownership
evidence. The directory is user-only and the Unix socket has mode 0600. The
address identifies the owner ID, random live instance, contract version, PID
and socket. A client verifies all live handshake identities, not just a path
or a product-version label. Ordinary reads never create a replacement owner.

This has the same local trust boundary as daemon IPC: processes of the same
OS user are trusted. The private channel must not become available to another
local user or a network peer. It is not an additional authorization boundary
against members running as that user. Environment snapshots and prepared inputs
travel only on this private channel; they must not be logged or persisted in
discovery metadata or execution-fact records.

## Control streams and admission

Requests are bounded newline-delimited JSON frames, at most 16 MiB including
the delimiter. Read-only Hello, Snapshot, Query, Find and terminal observations
do not acquire control. A separate terminal stream upgrades to the existing
terminal byte protocol and remains fenced by the active control epoch.

Each mutating connection acquires exactly one new epoch. Closing the previous
epoch, retaining its highest admitted sequence and publishing the replacement
epoch happen under the same admission mutex. A request decoded before takeover
must recheck the epoch under that mutex before admission. Dropping an old
connection cannot close the new epoch.

The controller assigns a strictly increasing positive sequence while actually
writing to that stream. An operation ID is `(owner instance, epoch, sequence)`.
Admission records the ID before spawning the execution task. The task belongs
to the owner, not the requesting socket; it must not block admission while a
provider turn runs. Retransmitting an admitted or reclaimed sequence cannot
execute it again.

Start carries a fully prepared launch and member creation identity. Each
launch receives the current controller's environment snapshot, rather than
implicitly inheriting an earlier controller's environment. Claude retains its
existing explicit environment allowlist. Stop, StopAll, Submit, generation-bound
controls and bounded terminal actions reuse the existing runtime mechanisms.
This channel does not dispatch arbitrary daemon operations.

For runtimes with a startup input gate, the primary PTY's Session retains the
gate across daemon controllers. Native startup confirmation, onboarding and
promotion screens MUST NOT receive automatic preambles, payload bytes or submit
keys. A verified chat input region or an explicit user release for the exact
PTY generation may pass the gate; reconnecting a controller or preserving the
owner MUST NOT reset it. A generation-bound release writes no native input.
Generic viewer/confirmation terminals and ACP sessions do not use this gate.

While a new PTY is still waiting, the controller may return a zero-input result
before admission only after excluding any existing source execution. A final
writer check remains necessary. An admitted operation that explicitly proves
zero submission may be reclaimed only after every source event's Unsubmitted
state is durably recorded. This permits the same pending work to retry; unknown
or partially written execution MUST retain its original non-replay semantics.

Hello reports whether Start remains open. StopAll permanently closes Start on
that instance; an unrelated new member then uses a fresh default owner. An old
member registration must still be confirmed stopped before it can move. The
closed owner remains discoverable for its existing resources and results.

New Start reuses the default owner only when its Hello reports both open Start
admission and the same known compiled source identity as the controller. The
source fingerprint covers execution code and resources, separately from Web
assets; it selects the implementation for a new launch, not protocol compatibility.
A changed or unknown source selects a new default owner after verifying no old
member registration would be replaced. A failed owner observation is an error,
not permission to assume its members absent. Existing members keep their actual
holder and execution code until explicitly stopped.

A controller with a known source also verifies the Hello from a newly spawned
owner before admitting its first member or browser. If replacing the program
at its launch path starts another build, the controller reaps that unused child
and reports that CCCC must restart before a new launch.

Once a replacement factory is ready, non-default owners may retire when their
last member and execution fact have been released. For an owner still accepting
Start, Retire MUST check both an empty member backend and no unsettled operations
under the same admission fence; pending or unacknowledged facts prevent retirement.
Acknowledgement, confirmed member Stop, controller startup and factory replacement
may retry this cleanup without a periodic sweeper. Only a positive Retired result
permits removing that client; failure does not undo a successful new Start. The
default empty factory remains available. After an explicit StopAll has permanently
closed Start, the existing whole-owner termination rule remains: an empty member
backend permits Retire even if execution facts remain. This does not turn an
unsettled delivery outcome into confirmed acceptance or authorize replay.

## Browser resources

The same holder may own a browser, its exclusive profile lease and any display
children. The executable supplies the existing browser launcher directly to the
holder entry. The daemon and Web do not hold the process or acquire a second
control epoch. No arbitrary command arguments or environment enter this API.

BrowserEnsure carries an absolute canonical profile, mode, dimensions and
normalized credential-free proxy settings. For a new browser the Web also
supplies its resolved program paths and desktop-view preference. A retained
browser keeps its original launch settings, while the current Web can disable
desktop access on its own connection. The controller checks every actual
holder before choosing its current launch factory: a source change must not
create a second browser for a retained profile. Existing profile/mode/proxy
must match; shared browsers retain the first launch's dimensions. A conflicting launch setting,
unavailable holder or pending resource is an explicit error. Launch reservation
precedes execution under the admission fence. A lost reply is queried or
observed; it does not authorize duplicate creation.

BrowserPagesSet replaces the complete mapping for one resource ID and holder
instance. Each entry contains a session key, optional member creation identity,
target ID and opaque Web state. The holder preserves that state but does not
interpret URLs, DOM, model turns or delivery settlement. Creating or replacing
a page publishes the mapping immediately, including outside an update relay.
Session metadata is bounded to 128 KiB per browser, launch and connection data
each to 16 KiB, and the holder admits at most 64 browser profiles, keeping a
complete resource list below the IPC frame bound.

The private owner Snapshot includes both process-member observations and
browser-resource observations. A browser member's generation includes its
resource ID and target ID, with the actual holder ID and member creation
identity. Browser liveness and a saved mapping establish retained resources;
they do not establish that the new Web can drive those pages. The holder records
the admitted operation's epoch on a successful BrowserPagesSet. Only a mapping
published in the current control epoch establishes Web reattachment. Takeover
retains old mappings for adoption but invalidates that prior connection fact.
An older publication cannot overwrite a mapping from a newer epoch. Web never
supplies or chooses the epoch. A retained login profile may remain unconfirmed
until it is opened and adopted; absence of that connection is not browser exit.

BrowserClose requires the resource ID and instance, uses the reserved stop
capacity and retains the handle when cleanup is unconfirmed. StopAll also closes
browser admission and stops browser/display resources. Both forms of Retire
require the browser registry to be empty, even when no member is registered.
A launcher error means nothing started or cleanup was confirmed; a process
whose initialization and cleanup failed is returned with its handle and error,
so an explicit StopAll can still retry cleanup. Presence of a Rust handle alone
is not process liveness: usable observations require the actual native identity.

BrowserResources is read-only, neither starts a factory nor acquires control.
It returns resource/holder/instance identity, normalized launch settings, CDP
connection data and the page mapping. Failed observation is not an empty set.
Each listed resource reports `pending`, `ready`, `exited` or `unconfirmed`;
only ready entries carry usable connection data. A failed or exited profile
does not hide the observations of unrelated profiles. Ensure may clean up an
actually exited resource on its original holder before launching a replacement;
it MUST NOT infer exit from an unreadable native identity. Native process
evidence stays in the holder. CDP endpoints and profile paths
belong only in private local IPC, never public health or MCP responses.

The complete preservation preflight includes all browser resources, including
profiles without member mappings. A pending or unconfirmed resource rejects
preservation, rather than disappearing as an empty set. Actual exited resources
are known absence. Browser member mappings cover only the matching member
creation identity in the configured cohort. StopAll waits for admitted browser
work to settle and reports the mapped members whose resource cleanup succeeded
or remained unconfirmed. Even an unmapped profile's failed cleanup prevents a
successful overall stop. Startup retention records resource takeover; later
observations require the current-epoch publication to report actual Web
reattachment. Positive target removal after complete adoption is absence;
an unavailable holder or resource remains unconfirmed.

## Execution facts and settlement

Submit includes the member creation/generation identity, ordered source event
IDs and a digest of the prepared input. The owner retains those references and
the actual terminal execution result; it does not create a second Inbox or
durable input journal. A daemon reconnects by member/source references and
queries facts instead of reconstructing a lost sequence or resending input.

Query returns one of:

| Result | Meaning |
| --- | --- |
| Pending | The owner admitted execution and still holds it. |
| Finished | The retained actual runtime outcome is available. Admission alone is not provider acceptance. |
| NotAdmitted | In this same live instance, the epoch is closed and the queried sequence exceeds its sealed admission waterline. |
| Unknown | There is no retained proof, including an absent ID below the waterline, expired detail or a changed live instance. |

Empty Find results are also unknown, never proof of non-admission. A query or
control failure must not authorize automatic replay. The daemon records the
actual result through its existing delivery/delegation ledger protocol. Only a
successful final ledger append permits acknowledging an execution fact. A new
epoch may acknowledge an old epoch's completed operation; repeated
acknowledgements succeed. Pending execution cannot be reclaimed this way.

A definitive provider failure may enter the daemon's existing bounded retry
policy. An attempted turn with an unknown result is settled Ambiguous before
any automatic wake, even if its native generation has exited. Observing an
in-flight claim does not register a second settlement worker. Each definite retry attempt records Failed, acknowledges that completed fact and
claims again before another native submission. Unsubmitted attempts likewise
need a fresh durable claim. Unknown outcomes cannot take this path. No separate
retry journal or automatic replay policy belongs to the owner. Between attempts,
the visible delivery state can briefly be Failed; the in-memory retry budget
does not survive a daemon restart during that backoff.

Ordinary operation retention is bounded to 4,096 slots and 32 MiB of admission
reservations/results. Completed records release the input reservation in favor
of their retained result/reference size. Closed-epoch metadata is bounded to
32 epochs and one hour. Finished detail outside the current epoch expires after
one hour; source-free lifecycle/control results expire after that interval even
in the current epoch. Pending work is never evicted. Missing/expired evidence remains
unknown. Capacity exhaustion explicitly rejects new work rather than erasing
an earlier input outcome.

Reclamation and stopping must remain possible at ordinary capacity. Ack and
Retire advance control sequence authority without allocating an execution
slot. Stop has a separate bounded pool, and StopAll has a reserved slot distinct
from member Stop slots. A pending duplicate Stop is rejected before consuming
another slot. Reclaimable completed Stop observations do not carry task-input
replay authority.

## Observations, exit and recovery

Finite synchronous list projections may explicitly share one snapshot per
owner instance within that request. Outside that scope, observations read the
owner directly. Writes invalidate the current request's projection. Wait/follow
loops and delivery decisions must not retain a scoped or time-based cache.

A member getter failure is retained with its identity as a private per-member
observation error. Other members can still be observed and reattached. Public
lists retain that member with `effective_working_state: "unknown"`,
`effective_working_reason: "runtime_owner_unavailable"` and a diagnostic
`runtime_observation_error` string, omitting unverified running/PID and actual
runtime observations. Other members remain independently observable. Group
projections report `runtime_status.available: false` and omit unknown liveness.
The same per-member signal is used in `actor.activity`; consumers clear old
observations instead of preserving stale running state. Decisions for another
member need not inherit a member-specific observation error.

Owner unavailability is not evidence of member exit. Lists and successful
configuration responses preserve unknown observations explicitly. Missing
fields and an unavailable Group projection must not trigger automatic Start.

The public maintenance request and final shutdown report are specified in
[CCCC_DAEMON_IPC_V1.md](CCCC_DAEMON_IPC_V1.md). Reports distinguish confirmed
stops, retained live members and unconfirmed ownership. A missing report is
not an empty successful result. With no daemon running, explicit Stop may take
the same exclusive home lock and contact discovered owners directly, without
starting routing, scheduling or providers. A held or unverifiable lock/socket
must not be treated as an empty registry. Process reclamation still requires
the existing recorded identity and confirmation policy; owner death or a closed
PTY alone does not prove every provider-created process has stopped.

Cloud-backed Dots and Grok Webhook routes are daemon-owned persistent resources,
not OS resources in this owner protocol. Maintenance checks their current Actor
identity, route revision/epoch and unfinished work from existing atomic stores.
No owner is invented for them. During preservation, active work is included in
the final report's `unconfirmed` set with no owner or process generation; startup
`retained_agents` remains an actual holder observation. An idle remote route has
no activity to add to either list. Unreadable or conflicting route facts refuse
preservation, and queries do not repair or initialize the connector document.
Post-restart queries still return independent local holder observations when
remote facts cannot be read; remote work in the stop receipt stays unconfirmed.
Failure/expiry and explicit turn resolution retain their existing provider
semantics; neither a saved route nor a failed callback proves cloud termination.
HTTP calls during host downtime can still fail. These limits and the recovery
rules are part of the public maintenance contract referenced above.

Startup and offline Stop hold the home lock and each dead owner's instance lock
while reusing the process guard's existing recovery rules. A verified boot
change clears old local work without signalling; same-boot process groups are
reclaimed only with recorded identity evidence. Unconfirmed evidence prevents
automatic restoration, and the address remains discoverable.

Claude background jobs can outlive their launcher and are not proved absent by
an empty process-group list. Before native launch, the owner therefore retains
a generic launch-intent record in the existing ownership ledger. This contains
no task, input, environment or credentials. A proved prelaunch refusal/no launch or the existing native Stop/rollback's
authoritative job-removal confirmation clears it. Missing control endpoints,
missing Session registration, or provider observation alone do not. Unknown
launch failures retain the record and prevent reporting false stopped state;
the unknown startup intent becomes reviewable while its owner remains alive. A failed new Actor launch
carrying a unique name may reuse an authoritative native listing of that exact
name: absence confirms that observation; a single valid matching job is rolled
back with native Stop. An unreadable or incomplete listing remains unknown.
This name rule does not apply to an existing job or a resume without that name.
A supervisor that creates the job after this listing remains a limitation.

After the daemon releases its home lock, the user can inspect
`cccc daemon process-recovery --owner session --owner-id <UUID> list` and reuse
the existing record/version-fenced `retry` or `accept --accept-risk` actions.
A live owner independently acquires that same home lock. Listing does not
acquire control; mutations use an epoch-fenced administrative operation without
another execution slot. A lost response requires listing the evidence again,
not replaying the mutation. Only an uncertain launch without an actual native
Session, PTY or trust/confirmation terminal may have its registration released
by risk acceptance. Healthy members sharing this owner remain untouched. The
user first exits the daemon with `daemon stop --keep-agents` to keep those
members running, reviews/accepts the one record, then starts CCCC to reattach.
An accepted record is not a confirmed stop; startup causes must be corrected
before another launch. For a dead owner, its instance lock and existing inactive
ledger recovery remain mandatory. All records must be resolved before a stale
address is retired. Ordinary Stop makes a best effort across every discovered
owner even if another cannot be contacted; missing complete ownership still
prevents reporting a successful complete stop.

## Contract versions

The contract number covers every message on the control socket and every type
reachable from one, wherever that type is defined, including field and variant
names. An optional field may be added under the same number when a peer of the
previous release that omits or ignores it still behaves correctly. Any other
change needs a new number: a renamed or removed field or variant, a new
required field, or a new meaning for an existing one.

An owner kept across an update runs the code of the release that started it
until it stops. A release lists in `owner_contracts` only the contracts whose
owners it can observe, control and stop. Before an attended update keeps
agents, the running daemon refuses when the candidate does not list the
contract of every running owner, and the offline installer check refuses
unless the staged program can reach every kept owner. Those agents then have
to be stopped by the release that started them. A daemon, offline stop or
recovery does not adopt, signal or stop a live owner of a contract it does not
implement; today it reports only an invalid discovery record. A release that
introduces a new contract MUST therefore either still implement the previous
one, or detect its owners and tell the user, for each, how the release that
started it stops it. Its release notes say whether agents keep running across
that update. Each release that lists the previous release's contract MUST be
checked by keeping real agents of that release across
`cccc daemon stop --keep-agents`, a restart into a local build and a release
update.
