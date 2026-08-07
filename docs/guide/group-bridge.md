# Group Bridge

Group Bridge connects trusted CCCC working groups across machines, networks, or teams. It is for explicit cross-group collaboration: one group can send messages to another group, and a trusted remote group can optionally inspect or operate on the target workspace through remote MCP tools.

Use it when you want a local-first group on one machine to coordinate with another local-first group elsewhere without merging their ledgers, actors, credentials, or runtime state.

## When to Use It

Good fits:

- A Mac group asks a Linux server group to run checks that only work on that server.
- A Windows group coordinates with a WSL group that owns the repository workspace.
- A lead group sends implementation or review tasks to worker groups on other machines.
- Two trusted teammates keep separate local CCCC instances but need durable, routed collaboration.

Not good fits:

- Public guest access. Group Bridge is a trust edge, not an anonymous collaboration feature.
- Simple mobile access to your own group. Use Web Access or an IM Bridge instead.
- Shared long-term knowledge storage. Use Group Space for provider-backed shared memory.

## Mental Model

A bridge has two independent directions:

- **Messages**: explicit cross-group messages. This is the safest baseline and the default collaboration path.
- **Remote access granted to them**: what the remote group may do in this group.
- **Remote access granted to you**: what your actors may do in the remote group.

The two access directions can differ. For example, your group can grant another group message-only access while that remote group grants your group read access.

Group Bridge preserves provenance. Relayed messages arrive with `source_platform=group_bridge_session`, a `group_bridge:<peer>` sender, and source group/event metadata so operators can trace where a message came from.

## Access Levels

| Level | What it allows |
|-------|----------------|
| **Messages** | Send explicit messages to the remote group. Use `@foreman` unless a specific remote actor is known. |
| **Read** | Inspect remote context, repository files, search results, and read-only git state. Does not wake target actors. |
| **Full** | Edit remote files and run remote commands through the same local-access surface used by native actors. This is not a sandbox. |

Access levels are cumulative: **Read** and **Full** retain explicit message delivery.

Keep bridges at **Messages** unless the current workflow needs more. Grant **Read** only to groups allowed to inspect the target workspace. Grant **Full** only to groups that may run commands and modify files in that workspace.

## Setup

Group Bridge pairing is managed in the Web UI.

1. Start CCCC on both machines:

   ```bash
   cccc
   ```

2. Make the issuer group's Web UI reachable by the requester for the pairing approval step.

   For local/LAN use, `http://HOST:8848` can be enough. For cross-network use, expose the Web UI through a protected HTTPS URL such as Cloudflare Tunnel, Tailscale Funnel, ngrok, or a reverse proxy.

3. In the issuer group, open **Settings > Group Bridge**.

4. Generate a one-time pairing invitation.

   The invitation is a JSON payload, not just a raw code. CCCC copies the full payload immediately and keeps it visible for manual copying if clipboard access is unavailable. Send the full payload to the requester. It expires and is shown once.

5. In the requester group, open **Settings > Group Bridge** and paste the pairing invitation.

6. Back in the issuer group, approve or reject the incoming request.

7. In either group, refresh the connection list and confirm that the remote group appears under connected remote groups.

After the first bridge setup, restart already-running actor runtimes once if you want them to see newly available remote read/full MCP tools.

### Python/Rust Upgrade Compatibility

The Rust service reads the legacy `group_bridge_identity.yaml`, `group_bridge_pairing.yaml`, `group_bridge_registrations.yaml`, and `group_bridge_credentials.yaml` files from the same CCCC home. Imports are idempotent and leave the legacy files unchanged, so switching branches does not require deleting or recreating bridge data. Existing peer identity is retained when legacy identity data is present.

Pairing and message delivery also support mixed Python/Rust peers. Rust accepts both pairing response shapes used by the two implementations and shares Python's Ed25519 signing identity file. The Rust daemon scans active session trusts, opens the same signed WebSocket used by Python, keeps it alive with heartbeats, and reconnects with bounded exponential backoff. Message delivery prefers this live route and falls back to authenticated HTTP and then the authorized remote MCP route.

An active pairing is authorization, not proof of reachability. A healthy Rust trust reports `session_connected=true`; `session_connected_at`, `session_last_error`, and `session_last_error_at` provide connection diagnostics. The remote endpoint must still be a non-empty HTTP(S) address reachable from the dialing peer.

## Sending Messages

Once paired, remote groups appear in the Web composer and in MCP group resolution. When `dst_group_id` is supplied and `to` is omitted, CCCC targets the remote group's unique available foreman. An explicit `to` always overrides that default; delivery fails closed when the target has no unique available foreman:

```text
to: @foreman
remote group: <remote_group_id>
message: Please run the release checks on the Linux workspace and reply with evidence.
```

For agent-driven messaging, use the normal CCCC message tools. Discover remote targets first:

```text
cccc_remote_access(action="list")
```

Then send a normal message with `dst_group_id` set to the remote group id and `to` set to `["@foreman"]`. For retryable workflows, reuse one stable `idempotency_key` so a transport retry does not create a duplicate remote message.

Attachments can be sent through Group Bridge when the target is a trusted remote group. Use attachments for evidence, logs, screenshots, or small artifacts that should be visible in the remote conversation.

Incoming remote messages preserve the source group, source actor, source event, and default return recipient. Reply with the delivered event's `reply_to` as usual; CCCC relays the reply to the originating group and keeps a local reply record. If the remote endpoint only exposes the legacy Group Bridge MCP surface, text delivery automatically falls back to that compatible path.

## Remote Read and Full Tools

Remote MCP tools are visible to agents when a bridge exists and the capability policy allows the Group Bridge pack.

Read tools:

| Tool | Use |
|------|-----|
| `cccc_remote_access` | List bridges, check access status, and explain permissions. |
| `cccc_remote_context` | Read the target group's context snapshot. |
| `cccc_remote_repo` | Inspect repository info, directories, files, and search results. |
| `cccc_remote_git` | Run read-only git `status`, `diff`, or `log`. |

Full tools:

| Tool | Use |
|------|-----|
| `cccc_remote_repo_edit` | Replace, write, move, delete, or create files in the remote active scope. |
| `cccc_remote_apply_patch` | Apply a Codex-style patch in the remote active scope. |
| `cccc_remote_shell` | Run a bounded one-shot command in the remote workspace. |
| `cccc_remote_exec_command` | Run a long-running command in the remote workspace. |
| `cccc_remote_write_stdin` | Poll, write to, or terminate a remote exec session. |

`cccc_remote_git` also allows mutation actions such as `add` and `commit` when the remote group grants **Full** access.
`cccc_remote_shell` accepts `timeout_s` from 1 to 600 seconds. Every session returned by
`cccc_remote_exec_command` is bound to the target group, registration, and active trust;
only the same authorized bridge may poll, write to, or terminate it.

All remote tools require `remote_group_id`. Use `cccc_remote_access(action="list")` to get the exact id and current permission level before calling them.

## Security Boundaries

Group Bridge is trust-based:

- Pair only with CCCC instances you control or explicitly trust.
- Treat a pairing invitation like a short-lived secret until it expires.
- Do not grant **Read** access to groups that should not inspect local repo files, context, or git state.
- Do not grant **Full** access unless the remote group may run commands and modify files in the target workspace.
- Path guardrails keep operations under the target active scope, but they are not a security sandbox.
- Before exposing Web UI beyond localhost, configure an Admin Access Token in **Settings > Web Access**.

Runtime state, credentials, and browser sessions remain local to each CCCC instance. The bridge does not merge ledgers or actor runtimes.

## Troubleshooting

| Symptom | Check |
|---------|-------|
| Pairing request cannot be submitted | The requester must paste the full JSON pairing invitation, and the issuer endpoint must be reachable from the requester. |
| Pairing code is invalid or expired | Generate a fresh pairing invitation from the issuer group. Raw codes are mainly for same-instance diagnostics. |
| Outbound remains `submitted` after approval | Refresh or sync the outbound record. Do not delete legacy YAML files; current Rust builds normalize older pairing responses and retain the existing request. |
| Pairing is active but `session_connected=false` | Verify that `remote_endpoint` is non-empty and reachable. Inspect `session_last_error`; the daemon retries automatically with exponential backoff. |
| Remote group does not appear in recipients | Refresh **Settings > Group Bridge**, confirm the trust is active, then refresh the Web UI group list. |
| Agents cannot see remote read/full tools | Restart already-running actor runtimes after setup and check the capability allowlist. |
| `bridge_remote_mcp_unavailable` | The bridge exists for messages, but the HTTP(S) remote MCP endpoint or token is not available. Refresh the bridge state and verify the remote endpoint. |
| Read/full calls are denied | The remote side has not granted that access level. Ask the remote operator to update the bridge access. |
| Remote command hangs or times out | Use `cccc_remote_exec_command` for long-running commands and poll the returned session with `cccc_remote_write_stdin`; keep `cccc_remote_shell` commands short. |

For collaboration semantics and cross-group provenance fields, see [CCCC Collaboration Standard v1](/standards/CCCS_V1).
