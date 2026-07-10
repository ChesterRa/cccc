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

   The invitation is a JSON payload, not just a raw code. Send the full payload to the requester. It expires and is shown once.

5. In the requester group, open **Settings > Group Bridge** and paste the pairing invitation.

6. Back in the issuer group, approve or reject the incoming request.

7. In either group, refresh the connection list and confirm that the remote group appears under connected remote groups.

After the first bridge setup, restart already-running actor runtimes once if you want them to see newly available remote read/full MCP tools.

## Sending Messages

Once paired, remote groups appear as explicit remote recipients in the Web composer and in MCP group resolution. Prefer sending to the remote foreman:

```text
to: @foreman
remote group: <remote_group_id>
message: Please run the release checks on the Linux workspace and reply with evidence.
```

For agent-driven messaging, use the normal CCCC message tools. Discover remote targets first:

```text
cccc_remote_access(action="list")
```

Then send a normal message with `dst_group_id` set to the remote group id and `to` set to `["@foreman"]`.

Attachments can be sent through Group Bridge when the target is a trusted remote group. Use attachments for evidence, logs, screenshots, or small artifacts that should be visible in the remote conversation.

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
| Remote group does not appear in recipients | Refresh **Settings > Group Bridge**, confirm the trust is active, then refresh the Web UI group list. |
| Agents cannot see remote read/full tools | Restart already-running actor runtimes after setup and check the capability allowlist. |
| `bridge_remote_mcp_unavailable` | The bridge exists for messages, but the HTTP(S) remote MCP endpoint or token is not available. Refresh the bridge state and verify the remote endpoint. |
| Read/full calls are denied | The remote side has not granted that access level. Ask the remote operator to update the bridge access. |
| Remote command hangs or times out | Use `cccc_remote_exec_command` for long-running commands and poll the returned session with `cccc_remote_write_stdin`; keep `cccc_remote_shell` commands short. |

For collaboration semantics and cross-group provenance fields, see [CCCC Collaboration Standard v1](/standards/CCCS_V1).
