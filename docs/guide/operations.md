# Operations Runbook

This page is for operators who need reliable day-to-day CCCC execution.

## 1) Runtime Topology

Default runtime home:
- `CCCC_HOME=~/.cccc`

Key paths:
- `~/.cccc/registry.json`
- `~/.cccc/daemon/ccccd.sock`
- `~/.cccc/daemon/ccccd.log`
- `~/.cccc/groups/<group_id>/group.yaml`
- `~/.cccc/groups/<group_id>/ledger.jsonl`

## 2) Startup and Health Checks

### Start

```bash
cccc              # interactive terminal: daemon + Web UI with the terminal interface
cccc --headless   # servers, containers, service managers, scripts: plain log output
```

Both run in the foreground and own what they started: quitting the interface,
Ctrl+C or a termination signal stops the Web server, and the daemon when that
launch started it. To keep CCCC running unattended, run `cccc --headless` under
a service manager such as systemd or a container runtime instead of detaching
it. Without a terminal, `cccc` runs headless on its own.

### Health Baseline

```bash
cccc doctor
cccc daemon status
cccc groups
```

Expected:
- daemon reachable
- runtimes detected
- active group list loadable

## 3) Incident Triage Order

When a group appears stuck:

1. Check daemon health.
2. Check group state (`active/idle/paused/stopped`).
3. Check agent runtime status.
4. Check runtime delivery, Inbox read state, pending replies, and tracked tasks.
5. Check automation and delivery policy.

Useful commands:

```bash
cccc daemon status
cccc actor list
cccc inbox --actor-id <actor_id>
cccc tail -n 100 -f
```

## 4) Fast Recovery Playbook

### Agent-level recovery (preferred) {#actor-level-recovery-preferred}

```bash
cccc actor restart <actor_id>
```

Use this before group-level restart.

### Group-level recovery

```bash
cccc group stop
cccc group start
```

### Daemon-level recovery (last resort)

```bash
cccc daemon stop
cccc daemon start
```

### What a daemon restart keeps {#what-a-daemon-restart-keeps}

A normal `cccc daemon stop` stops the agent processes that CCCC started and says which
ones it stopped, naming any agent it could not confirm as stopped. Sessions you started
yourself keep running. On Linux and Apple Silicon Macs running the native arm64 build,
`cccc daemon stop --keep-agents` also keeps managed agents running. See
[Keeping agents across a restart](#keeping-agents-across-a-restart).

| Part | After the daemon starts again |
| --- | --- |
| Groups, messages, tasks, context, automation rules | Kept; they are files in `CCCC_HOME`. Each Group keeps its state, and the agents of a stopped Group are not started. |
| Message delivery | Delivery, read and reply records are kept. Send messages not yet handed over are delivered. A handoff without a recorded result becomes uncertain and is not resent; see [Delivery and recovery](runtimes.md#delivery-and-recovery). |
| Agents CCCC manages | CCCC tries to start the enabled agents of a running Group again; in a paused Group, headless agents are not started. The live terminal, its input mode and its output buffer are not carried over, and a turn that was running does not continue. Whether an agent picks up its previous conversation depends on its [runtime](runtimes.md). With durable persistence on, earlier terminal output stays readable ([Terminal history](terminal-history.md#cursor-and-restart-boundaries)). |
| [Sessions you started](local-sessions.md) | They keep running, but CCCC tool calls fail while the daemon is down. Their receive channel reconnects after start, and messages waiting for them are delivered. |
| Automation | Rules and their schedule clocks are kept. An enabled rule whose clock fell due while the daemon was down fires once, on the first scheduler tick that can carry it out: nothing runs while its Group is paused or stopped, and a notification with no enabled recipient waits. Resuming a paused Group resets the clocks, so runs missed while it was paused are skipped. |
| [Voice secretary](voice-secretary.md) | Its shared conversation is not restored. |
| [CCCC Connect](connect.md) | Pairing, queued messages and bounded retries continue. |

### Keeping agents across a restart {#keeping-agents-across-a-restart}

If you are upgrading from 0.4.58, let active turns finish first. Its updater
stops the agents it manages before replacing CCCC. When you start CCCC with
the same home, your Groups, member settings, tasks, Group message history and
unread Mail are still there. Later updates can keep eligible agents running.

On Linux and Apple Silicon Macs running the native arm64 build,
`cccc daemon stop --keep-agents` stops only the daemon. The agents CCCC
started keep running with their terminal and output, and a turn that was running
continues. `cccc daemon start` lists the members it found. Web reconnects to
their retained browser pages when it starts. A message whose
delivery was in progress is settled after the start and is not sent twice.

- If the daemon crashes, its agents keep running in the same way and the next start
  reattaches them.
- For a planned stop, use `--keep-agents` to keep agents. A plain `cccc daemon stop`,
  or closing the terminal that runs `cccc daemon run`, stops them as before.
- To update without stopping agents, run `cccc daemon stop --keep-agents`, then
  `cccc update`, then `cccc daemon start`. `cccc update` on its own restarts a running
  daemon, which stops its agents.
- After such an update, the kept agents stay with the process that has been holding
  them. New agents start under the new build. A plain `cccc daemon stop` stops
  agents held by either build.
- ChatGPT Dots and Grok Bot Web Model agents run in the provider's cloud. CCCC
  preserves their saved connection and its record of unfinished work. The update
  result lists an agent with unfinished work as unconfirmed because CCCC cannot
  see whether the provider continued the turn.
  While CCCC restarts, its address does not answer, so a CCCC tool call from that run can
  fail or end it. Check the turn's result before sending its input again.
  If CCCC cannot read the saved cloud connection or determine whether a turn is
  unfinished, choosing to keep agents in the Web or terminal interface stops the
  update before shutdown. The command `cccc daemon stop --keep-agents` checks
  those records after releasing control of the local agents. If this check fails,
  the command reports that it could not confirm which agents kept running.
  Start CCCC again to see which local agents it finds.
- On Windows and with an Intel macOS build, the daemon rejects `--keep-agents`
  before stopping any agents.

After such a restart, CCCC can use two session owners for a while: the earlier
one holds the kept agents and the new one holds agents started afterwards. If
one of them stops responding while CCCC runs, running agents whose healthy owner
CCCC can verify keep receiving new messages and can be stopped individually.
Messages to the unresponsive owner's agents that were not yet handed off can
remain pending and be sent after it responds, and stopping the whole Group waits
for both owners. Managed and DeepSeek agents kept running from v0.4.59 get this
only after they are restarted. If you need the healthy owner's agents to keep
working, keep the daemon running: a plain `cccc daemon stop` also stops them,
and startup can then fail and name the unresponsive owner. The
[v0.4.60 release notes](../release/v0.4.60_release_notes.md#known-limitation)
give the steps for that case.

If you are returning to a version that cannot reattach running agents, let
active work finish. If you stopped CCCC with `--keep-agents`, first use the
newer executable and the same `CCCC_HOME` to run `cccc daemon start` and
reattach the agents.

- With the current website installer, leave the newer daemon running. The
  installer stops it and checks that the managed agents exited before
  replacing CCCC. If the installer says the older release cannot check for
  agents left running without a daemon, start the newer daemon and retry.
- For a manual replacement, run `cccc daemon stop` with the newer executable
  and confirm the managed agents exited before replacing it. An older version
  can start a second copy of an agent that is still running.

### Restarting CCCC without checking for a release

On Linux and macOS, when the CCCC host started its own daemon, open
**Settings > This instance > Update** and select **Restart CCCC**, or type
`/restart` in the terminal interface. The confirmation shows whether CCCC
will use the current program or a new build at its launch path. Choose
**Keep agents running** where offered, or **Restart agents** to restart them
along with CCCC. In the terminal, the choices are **Keep agents and restart
CCCC** (where offered) and **Restart agents with CCCC**. The Web reconnects
at the same address and shows the result.

To run multiple instances on one machine, use a distinct `CCCC_HOME` and Web
port for each. A restart only reconnects to sessions in its own home. Instances
launched from the same executable path share the installation on disk. When
one updates that installation, the others can still serve from their running
builds and will use the shared file on their next launch.

### Starting after an interrupted local restart

If a local restart leaves CCCC down, start a working CCCC executable
with the same `CCCC_HOME`. Once CCCC verifies that the previous update host has
exited and takes the daemon lock, it reports the interrupted restart as abandoned
and starts the executable you chose. On platforms that support keeping agents, the
daemon can reconnect to agents that stayed running during the interrupted restart.

If another CCCC process holds the daemon lock, wait and retry. Run
`cccc daemon status` if the lock stays busy. For an interrupted release
installation, use the `cccc update --recover` command that CCCC prints.

After a release update, the new CCCC can be running even if the installer could
not confirm the update. In the terminal interface, the recovery command is in
`CCCC_HOME/daemon/cccc-web.log`. With `cccc --headless` or `cccc web`, it is
printed on standard error. Stop CCCC before running the command and use the
same `CCCC_HOME`. If the installer still has the previous executable, recovery
restores it. If the installer has already discarded the backup, recovery checks
the installed build and keeps it if it matches the update.

### When CCCC cannot confirm an agent {#when-cccc-cannot-confirm-an-agent}

On platforms that support keeping agents, an agent's launch can end before CCCC
can confirm whether its work is still running, for example when Claude Code fails
in a way CCCC does not recognise. The agent then
shows as Unknown, its tooltip gives the review command, and `cccc daemon stop` names it with
that command, `cccc daemon process-recovery --owner session --owner-id <owner-id> list`. The
other agents and the lists keep working.

1. Run `cccc daemon stop --keep-agents`, so the other agents keep running, then the `list`
   command. It needs the daemon to be stopped.
2. Check that the agent's work has ended, then run
   `cccc daemon process-recovery --owner session --owner-id <owner-id> accept --id <id> --version <version> --accept-risk`,
   with the `id` and `version` that `list` shows. This releases only that agent. It does not
   stop anything.
3. Fix what made the launch fail, then run `cccc daemon start`. It reattaches the agents that
   kept running and, in a running Group, starts that agent again: if the cause is still there,
   the agent is Unknown again.

A plain `cccc daemon stop` can also fail to confirm a running agent, for example when its
terminal does not answer in time. It then says to run `cccc daemon stop` again: with CCCC
stopped, that run reaches the agent's session owner directly and stops the agent once it
answers. The review command comes second: `list` shows the owner's retained recovery
records, and an agent the owner still runs may not appear there. After
`cccc daemon stop --keep-agents`, use only the review command, because a plain stop would
also stop the agents it kept.

## 5) Secure Remote Access

Required baseline:
- Create an **Admin Access Token** in **Settings > This instance > Web Access** before any non-local exposure.
- Use Cloudflare Access or Tailscale for network boundary.

Do not:
- Expose Web UI directly without an access gateway.
- Store secrets in repo files.

## 6) Upgrade Playbook (RC-safe)

### Before upgrade

1. Stop active high-risk sessions.
2. Backup `CCCC_HOME`.
3. Record current version and smoke state.

### Upgrade

```bash
# Website-installer ownership
cccc update

# Pip ownership
python -m pip install -U "cccc-pair>=0.4.36"
```

To update from CCCC itself, open **Settings > This instance > Update** in the Web, or
type `/update` in the terminal interface that `cccc` starts. CCCC looks for a newer
release only when asked. It shows the installed and latest versions with a link to the
release notes, and says so when the check fails. On Linux and Apple Silicon Macs,
when the website installer installed CCCC and the `cccc` or `cccc web` that
serves this Web also started its daemon, select **Update now** in the Web and
choose how to handle your agents.

- **Keep agents running** downloads the release and checks whether CCCC can keep
  the current agents. If the check fails, the current instance and its agents
  continue running. See [Keeping agents across a restart](#keeping-agents-across-a-restart).
- **Restart agents** stops the managed agents for the update. When CCCC starts
  again, it starts eligible agents according to their Group and member settings.
  See [What a daemon restart keeps](#what-a-daemon-restart-keeps).

In the terminal interface, select **Keep agents; update** or **Update and
restart agents** from the `/update` menu.

During the update, the Web disconnects. It shows the result when CCCC returns
at the same address. A host that offers only a restart shows **Update and
restart agents**. Where an in-place update is unavailable, the page shows the
command for this installation or asks you to update CCCC the way it was installed.

Do not remove the v0.4.36 lower bound. Before a pip upgrade, run
`cccc daemon stop` and close any
foreground CCCC process so the executable is replaceable on every platform.
Do not layer the website installer over a pip-owned command. To switch
installation channels in the same directory, uninstall `cccc-pair` with pip
first; the installer refuses `pip-v1` ownership even when
`CCCC_ALLOW_REPLACE_EXISTING=1` is set.

### After upgrade

```bash
cccc doctor
cccc daemon status
cccc mcp
```

Run a small end-to-end smoke:
- create/attach group
- add/start agent
- send/reply
- verify ledger and inbox behavior

## 7) Backup and Restore

### Backup (minimal)

Backup `CCCC_HOME`:
- registry
- daemon logs (optional)
- all groups (`group.yaml`, ledger, state)

### Restore

1. Stop daemon.
2. Restore `CCCC_HOME` directory.
3. Start daemon and verify with `cccc doctor`.

## 8) Operational Guardrails

- Keep one source of truth: decisions should be in CCCC messages.
- Use `message_mode=request_reply` only for a concrete recipient whose reply is
  required; use Mail for useful but non-urgent context.
- Mail is agent-only. Address either `user` alone or one/more agents in each
  message; split messages instead of mixing those audiences.
- Prefer explicit recipients over broad broadcast when scope is narrow.
- Keep automation focused on objective reminders, not chat noise.

A new Mail sent within the same Group to explicitly named agents can return
`mail_recipient_advisories` with a `mail_advisory` explanation. These report
the daemon's snapshot when the Mail was written: an agent may be disabled, its
Group paused or stopped, or its managed runtime reported as not running. Mail
does not start an agent or resume a Group. Send, or delivery of the existing
Mail, can request delivery to an enabled agent in an active Group; delivery can
still fail or remain unconfirmed.

The optional result does not assess the running state of native sessions or Web
Models, and it is not returned for recipient selectors, retries, cross-Group or
Connect sends. A result without the field does not mean the Mail was read.

## 9) Escalation Checklist

If an issue repeats:

1. Collect evidence:
   - group id
   - agent ID
   - event ids
   - recent `cccc tail -n 100`
2. Capture reproducible sequence.
3. Classify severity (`P0/P1/P2`).
4. Register fix or risk in release findings.

## 10) Group Space (NotebookLM) Runbook

### Activate the provider

Connect Google from the Notebook settings in CCCC Web. That flow stores the
credential and provider state used by the daemon. No feature toggle or
environment variable is required.

### Validate control plane

```bash
cccc space credential status
cccc space health
```

### Validate explicit local-file ingestion

Create a supported file under the attached scope and ingest it explicitly.
The native daemon rejects paths outside that scope before any provider write.

```bash
cccc space ingest --kind resource_ingest \
  --payload '{"source_type":"file","file_path":"space/spec.md","title":"Spec"}'
```

Expected: the result contains the created NotebookLM `source_id`.

### Disconnect safely (core workflows keep running)

Use **Disconnect Google** in the Notebook settings to remove this machine's
stored credential and disable provider access from this installation.

Expected after disconnect:

- Group Space operations may return degraded/disabled provider results.
- Core CCCC chat, task and agent workflows continue normally.
