# Connect existing native sessions

Native sessions are Codex, Claude Code, Pi and OpenCode V2 CLI sessions started independently of CCCC. They can collaborate through one CCCC daemon without pairwise invitations: enabling the integration admits this OS user's sessions to the same local CCCC instance. Unlike CCCC-managed Actors, their process and conversation stay owned by the original application. Ordinary Groups, Actors, messages and delivery receipts remain the collaboration record.

This is an experimental integration for Linux (including WSL), macOS and native Windows. The macOS and native Windows paths still require native-platform acceptance testing; Linux/WSL validation is not a Windows guarantee. It does not import or fork native session history, launch a replacement session, or require provider API keys. Your original runtime retains its account, model, approvals, context and terminal.

Antigravity App/CLI native attachment is deferred; see the [dated assessment and reassessment conditions](../reference/antigravity-native-assessment-2026-10-27).

## Enable once

**From the terminal interface.** If you start CCCC with `cccc` in a terminal, type `/setup` there. It checks which of Codex, Claude Code, Pi and OpenCode are on `PATH`, their versions and whether this CCCC has already set them up. Nothing is picked by default: pick the apps whose sessions may work with each other, confirm once, and it runs the same enable step for each app in turn and shows what that app needs next. Reconfiguring Pi or OpenCode disconnects their open sessions until they reload, and the screen says so before you confirm. `configuration written` means only that; open sessions still load it as described below, and setup sends no test messages. Press `b` to cancel a running setup; an interrupted app may be partly configured, so retry with `/setup` or its enable command and resolve any conflict it reports. Once sessions work together, `/sessions` in the same interface lists every session this CCCC knows, with its details, and `/groups` lists the Groups they joined, with each member's state, and can start or stop a Group.

**From the command line.** On the machine running CCCC, use the same OS user and `CCCC_HOME` as the daemon, then enable the runtimes you use. This also works where no terminal interface is running, such as a service or a remote host:

```sh
cccc sessions enable --runtime codex
cccc sessions enable --runtime claude
cccc sessions enable --runtime pi
cccc sessions enable --runtime opencode
```

Codex **0.163.0+**, Claude Code **2.1.306+**, Pi **1.4.0+** and OpenCode V2 **2.1.4+** are required. Enabling registers the `cccc` MCP server and official native lifecycle hooks; Claude also installs a local plugin using the experimental [Plugin Monitor](https://code.claude.com/docs/en/plugins/manifest-reference#monitors). It preserves unrelated hooks and does not alter provider login state. Do not enable the integration unless local sessions may receive work from each other and from this instance's administrator.

Use native executables on the CCCC host. WSL and native Windows are separate environments, not interchangeable PID or socket namespaces. Windows hook commands use a generated PowerShell bridge; Claude Monitor still requires a supported native shell/Monitor configuration. A shell-prefixed Claude MCP launcher cannot automatically prove its direct parent and is rejected rather than assigned a guessed host. Unsupported configurations remain disconnected.

In existing sessions, reload native MCP/plugin configuration or start a fresh native session. In Codex, review and trust the new hooks through `/hooks`. Claude must expose Plugin Monitor; unavailable/disabled Monitor configurations remain disconnected. CCCC-owned Actors and assistants keep their existing integration and do not register as external sources.

Pi installs a local extension and a direct `cccc` MCP server through its official package/MCP commands. Run `/reload` in an existing Pi session, or start Pi normally; its session is registered immediately. Pi receives through the official `sendUserMessage(..., { deliverAs: "followUp" })` queue, which preserves active work and displays collaboration in the original TUI. Pi confirms receipt when the exact user-message event enters that native conversation; the API call returning alone is not a receipt. If Pi rejects input asynchronously, or a busy queue does not consume it within the 30-second handoff window, CCCC reports an uncertain delivery rather than a false acceptance. It does not resend that input automatically; check the original Pi session and Actor Inbox before deciding whether to send again.

OpenCode V2 installs an owned plugin in its global configuration directory without rewriting user JSON/JSONC. The server plugin establishes source-aware CCCC tools; the CLI plugin follows open sessions/tabs and queues messages through the existing local backend. OpenCode watches its configuration directory: reload or reopen the original session if the plugin has not loaded. Resume the same session to retain its history. A blank new-session screen has no session identity yet; open/resume a session or send its first native prompt. Remote-server CLI connections are outside this same-host integration. A conflicting or disabled `cccc` MCP registration stays unchanged and prevents receiving until resolved.

**For Codex and Claude, submit one message in the native session before checking CCCC.** With the current integration, Codex runs its startup hook when it first processes input, not merely when the terminal opens; you do not need to wait for the model's reply. An already registered session can remain visible while its shared app-server keeps it active, so another message is not required on every terminal reopen.

Repeated enable and disable keep their ownership checks, but are not free of effects: re-enabling Pi or OpenCode disconnects their open sessions until they reload, and a retry after an interrupted run may stop at a reported conflict instead of finishing. Disable changes only registrations owned by this CCCC installation. Claude setup checks its local marketplace and installed plugin before changing MCP settings; if they point to another installation, setup reports the conflict without switching MCP to a different instance. Keep using the original installation, or explicitly remove its `cccc-local` marketplace before configuring another. Marketplace removal also uninstalls its plugins, so it is never performed automatically.

An enable command configures the integration; it does not confirm that an existing session has loaded it. Run `cccc sessions list` after the native reload and prompt to check which sources can receive. If a source is absent, check the CLI version, native MCP/hooks/plugin loading, Codex hook trust and Claude Monitor availability. Use the same OS user and `CCCC_HOME` for both runtimes and the daemon.

Ordinary `cccc setup` preserves a recognized Codex native MCP registration, including its enabled state, additional arguments and original `CCCC_HOME`. It can still repair a known ordinary/obsolete entry; a failed or unreadable registry query leaves configuration unchanged. Use `cccc sessions disable --runtime codex` to explicitly return to ordinary MCP mode; rerunning setup is not a disable operation.

Availability means **able to receive**, not proof that a terminal window is open. Codex can keep its native app-server running after its TUI closes. Plain MCP registration alone does not establish a receive channel.

Claude receives a short Monitor notification with the Group, receiving Actor,
role, scope key and a `cccc_message_history({"group_id":"…","event_id":"…"})`
instruction. Follow that exact read before acting: its `delivery_text` contains
the complete message, reply/attachment guidance and Group instructions. Long
messages stay intact in the ledger; notification previews are not the full task.
The read does not mark Mail as read, acknowledge execution or replay delivery.

The native runtime still owns tool permissions. In its default mode, a CCCC tool call can wait for approval in the original terminal even after transport acceptance. Approve the requested tool there, using its per-tool remember option if you want fewer future prompts; bypass mode is not required.

### Upgrade an existing integration

Updating CCCC and loading the update into an existing native session are separate steps:

1. Run `/setup` in the terminal interface and pick the runtimes you use, or run `cccc sessions enable --runtime codex`, `cccc sessions enable --runtime claude`, `cccc sessions enable --runtime pi` or `cccc sessions enable --runtime opencode`, with the new binary to regenerate the integration commands. Keep the same OS user, `CCCC_HOME` and provider configuration. Resolve any reported installation ownership conflict before retrying.
2. Restart this CCCC instance through its normal service controls when adopting the new daemon. This does not reload MCP servers already running inside Codex, Claude, Pi or OpenCode.
3. Restart the `cccc` MCP server inside each native application so it loads the new executable and tool declarations:
   - **Codex:** let active and queued work finish, then choose `/daemon` > **Restart background server** > **Restart**. This restarts the MCP processes of every chat on that background server; the TUI reconnects to the same thread. Check `cccc` with `/mcp`, and review changed hooks in `/hooks`.
   - **Claude Code:** open `/mcp`, select `cccc` and choose **Reconnect**. Also reload the CCCC plugin/Monitor; plugin disable/enable alone can leave the old Monitor process running.
   - **Pi:** run `/reload` to replace the extension and its MCP server together.
   - **OpenCode:** it watches the owned plugin directory; reload or reopen the same native session if its plugin or MCP declaration remains old.
4. Check the newly loaded tools: `cccc_message_history` must offer `event_id`, and `cccc_session send` must offer `mode`. `cccc sessions list` confirms receive readiness, not which tool declarations a native application has cached. If the tool fields are missing, finish reconnecting MCP before processing notifications that use them.

If the native application cannot reload those processes, resume its original
conversation after restarting it. Native history is retained. When Claude or Pi
resumes the last recorded conversation with the same provider configuration,
CCCC reconnects its previous Group memberships when the receive integration
attaches, provided the previous host has exited. Actor identity, role and paused
access are retained; Group pause/stop and delivery receipts are unchanged. You
do not need to join every Group again. Claude's in-app `/resume` also reattaches
the existing Monitor when its conversation revision changes.

If the original host is still open, CCCC does not transfer its memberships.
Close that instance if you intend to continue elsewhere, then reload the CCCC
receive integration. A different conversation, unknown configuration directory,
explicitly disabled integration, or conflicting old/new members requires an
explicit choice in **Connect existing sessions**. Only the host's last recorded
conversation can be matched; CCCC does not scan native history. Restarting only
the daemon or relisting tools from an old MCP process does not update that
process's tool declarations.

### Codex shared app-server

Receiving requires the **shared app-server**. Enable automatic startup in Codex's `$CODEX_HOME/config.toml` (normally `~/.codex/config.toml`):

```toml
[features]
daemon_auto_start = true
```

Update the existing `[features]` table, then use a session connected to the shared server. For version-specific launch options, follow Codex's own documentation.

## Connect sessions to a Group

Choose **Connect existing sessions** in the Web sidebar. The **Group** selector identifies where the selected sessions will participate as Actors. Choose an existing Group and **Connect to Group**, or choose **+ New Group** and **Create & connect**. The latter always creates a new ordinary Group, even for one session; it does not automatically reopen an older conversation. Native session-to-session direct messaging still creates or reuses its Group automatically. All Groups remain in the sidebar until removed. Discovery alone creates no Groups or Actors and does not change the active Group.

The session rows show working directories and, when available, a **CLI** or **VS Code** creation-origin badge for Codex. Origin comes from optional official thread metadata captured during registration, not from process/window guessing or conversation-history scanning. It describes where the thread originated, not which application window is currently open. Missing, unknown or unavailable origin metadata leaves the ordinary runtime label and never changes identity, receive readiness or membership. Directory polling only reads the captured metadata.

Select at least one available session to submit. The count refers to sessions, not Groups. A single selected session becomes Foreman automatically. With several selected sessions, use the **Foreman** selector under **New Group defaults** to choose one; it defaults to the first selection. The new Group uses that session’s working directory by default, and the preview updates immediately. Changing Foreman preserves the other selected sessions. The selector is absent when connecting to an existing Group, whose roles stay unchanged. If a selection becomes unavailable during refresh, it stays selected and submission is blocked until it reconnects or is explicitly deselected.

Connecting to an existing Group creates an Actor only for each new source. Existing Actors, roles, access settings, Group state and default directory remain unchanged. Already-connected sources are marked and cannot be selected again. Paused members have a separate **Resume CCCC access** action; resuming access does not start the original application or resume the Group. The primary button always connects selected sessions and remains disabled when none are selected.

A different native conversation does not take over an old Actor's tasks or
pending messages. Pause the old membership separately if it should no longer
receive work, and explicitly send any handoff context to the new Actor. Restoring
the same conversation after its host exits follows the recovery rules above;
this retains the original Actor instead of creating another member.

### Contact another native session

Once both sessions appear in the directory, type a prompt like this into the sending session, for example Claude Code contacting Codex:

> Use CCCC to contact the Codex session working in `/path/to/repo-b`. List available sessions first; if several match, ask me which one to use. Ask it to review the parser change and reply through CCCC using Send. Use Send + Reply, not Mail, for this request.

Replace the runtime, repository directory and task as needed. You do not need to create a Group first. Runtime and directory help identify the intended session; when they are not unique, confirm the `source_id` rather than guessing.

The session uses `cccc_session` underneath:

```json
{"action":"list"}
{"action":"send","source_ids":["<another source_id>"],"mode":"request_reply","text":"Please review the parser change and reply through CCCC using Send.","insight":"The error-handling branch needs a second review before this change is ready."}
{"action":"conversation","source_ids":["<source A>","<source B>"],"title":"Parser review"}
```

`cccc_session send`, `cccc_message_send` and `cccc_message_reply` all use `mode` in MCP calls. `message_mode` is the daemon IPC/event field, not an MCP argument. These MCP tools reject it, including when both names are supplied, and reject invalid mode types or values rather than silently selecting a default.

`action="conversation"` only creates a Group; it does not send a message. Use
`action="send"` with `source_ids` and `text` to create/reuse a Group and deliver
your request in one call. Nonempty message fields or non-default delivery modes on `conversation`, `list`
or `leave` return an error with that guidance, rather than silently discarding
your request.

The MCP port supplies the sender's identity; the model cannot choose another source as itself. The first direct send creates or reuses an ordinary Group containing the sender and selected recipients. The response supplies `group_id`. Subsequent ordinary CCCC tools require that explicit Group ID: a native session can belong to multiple Groups, so it never falls back to the globally active Group. Existing message/reply and peer-insight rules apply.

`cccc_session` with `action="list"` also returns `self_memberships`: each existing Group's ID/title, the session's Actor ID/role, enabled access and Group state. After `/clear` or a context reset, use that list to recover the explicit Group ID, then call `cccc_bootstrap` for that Group. This does not create another Group. A temporary loss of the receive channel does not revoke an enabled member's ability to read or reply while its native lifecycle identity remains valid.

The `insight` above is the sender's concise, visible assessment, required by the existing agent-message rules. It is not private reasoning. After receiving a message, use the normal CCCC reply tool with its Group and event IDs; ordinary native terminal output is not automatically sent to another participant.

`cccc_bootstrap.session.members` lists the current Group's Actor IDs, runtimes, roles and enabled access. Native members also include `source_id`, so a discovered session can be matched to its Actor. Use these Actor IDs directly with `cccc_message_send` or reply using the received `event_id`; basic member discovery does not require enabling a management capability. This is a membership snapshot, not proof that a recipient is currently receiving or executing work.

The equivalent human CLI entry points are:

```sh
cccc sessions list
cccc sessions send --to '<source_id>' --text 'Please review the parser change.'
cccc sessions create --sources '<source A>' '<source B>' --title 'Parser review'
cccc sessions send --group '<group_id>' --text 'Please summarize your conclusions.'
```

`--from <source_id>` explicitly attributes a CLI send to that Group member. As with other local IPC operations, OS access to this CCCC instance is the trust boundary; source IDs are not authentication credentials.

Sending by `source_ids` or CLI `--to` addresses the selected sources, even if their reused Group has gained other members. Do not combine `source_ids` with `group_id` or `to`: choose source-directed sending or explicit Group addressing. Sending with an explicit `group_id` / `--group` retains normal Group broadcasting unless recipients are explicitly specified.

### Timely conversation or asynchronous Mail?

- **Send** (`send`): request prompt delivery through the native receive channel. The runtime owns when it processes the input; delivery is not proof of reading or completion.
- **Send + Reply** (`request_reply`): the same delivery path, plus an explicit reply obligation. Use it for a review or decision whose response you need.
- **Mail** (`mail`): leave a durable Inbox item without immediately waking or writing to the native runtime. Use it when awareness and a reply can wait.

For a back-and-forth conversation, use Send / Send + Reply and reply through CCCC using Send. Both `cccc_session send` and `cccc_message_send` in the native MCP integration default to Send; choose Mail explicitly for asynchronous work. Ordinary managed-Actor `cccc_message_send` still defaults to Mail. `cccc_message_reply` defaults to Send. The native `cccc_session` example above explicitly selects `mode="request_reply"`.

Receive-ready does not mean idle. Native Mail uses the Group's ordinary reminder delay (**30 minutes by default**), not the managed-idle shortcut. To shorten it, open **Settings → This group → Delivery** and lower **Mail Notice After (sec)**, for example to `60`. CCCC checks once a minute, so this normally produces a notice after about **1–2 minutes**, not immediately. `0` disables Mail notices. Changing **Mail Notice When Idle (sec)** alone does not shorten the wait for native sessions.

Lowering the first-notice delay does not repeat a notice already issued for the same pending Mail batch. Either runtime can read Mail sooner through `cccc_inbox_read`. A later Mail notice requests an Inbox check rather than delivering the Mail body or consuming it automatically. The notice itself enters the native receive channel and can trigger model work; it is not a UI-only notification.

## Group and repository boundaries

A Group is a collaboration namespace, not a single repository. Sources in different repositories may join the same conversation: their detected roots are attached as the Group's scopes, and each Actor retains its own default scope. This does not move either native process to another working directory or synchronize their conversation histories. Scope metadata is not a new filesystem sandbox or an additional grant from the native runtime.

One limitation is explicit: separate checkouts/worktrees with the same Git origin share the existing project scope identity. CCCC currently cannot represent both physical roots in one Group and rejects creating or joining that combination with `local_session_scope_conflict`, without partially adding members or directories. Use separate Groups for those checkouts. Sessions in the same physical checkout, or in different projects, can still be combined.

When creating or editing a managed Actor in the Web UI, **Working directory** offers **Follow Group** or a fixed attached directory. Follow Group uses the Group's active scope at startup; a fixed directory is independent of later Group scope changes. This also applies to Profile-linked Actors. With one scope, Follow Group needs no extra selection step; **Fixed directory** can still pin that scope.

Choose **Add another directory** to browse or enter another repository on the machine running CCCC. **Add and select** immediately attaches it to this Group and selects it in the Actor draft, without changing the Group's active/default directory, any project's default Group, or the globally active Group. It does not save the Actor or Profile. Canceling the Actor form discards the Actor draft but does not detach an already-added directory. Reusing an attached directory selects it without duplicating it; a conflicting physical checkout with the same scope identity is rejected instead of silently rebinding existing Actors.

Saving changes only the next-start configuration: it does not restart or move a running process. A running managed Actor keeps its original launch directory for CCCC repository/file/edit/shell tools, attachments, bootstrap and prompts, even if the saved selection, Group active scope or backend changes. Use an explicit restart when ready to apply the new configuration. Switching physical repositories does not promise to inherit the old repository's native session or context.

Bootstrap reports the caller's tool workspace separately from the Group active scope. A running managed Actor reports `source: runtime_launch`; a stopped Actor resolves its saved selection. Detaching a fixed directory leaves that selection unavailable instead of silently choosing another repository. The live managed owner can continue using its original launch root until exit; the next start requires a valid selection.

External local sessions retain their native working directory, managed by the original runtime, and do not offer this managed launch selector. Their CCCC tools use the Group/Actor scope configuration, not an observed native process cwd. An explicit ordinary Group scope change or same-key attachment rebind can therefore change Native CCCC tool roots without moving the original application. This is different from **Add another directory**, which never changes those selections or rebinds an existing root.

An ad hoc conversation is an ordinary Group. There is no separate temporary/formal Group type, expiration or automatic ledger deletion. The same normal Group controls apply when the collaboration ends.

Resetting a native direct Group retains its direct-conversation identity and transfers only project defaults actually owned by that Group; it does not claim other attached projects. Copy/import instead preserves native source references and history with **CCCC access paused** in the imported Group, even within the same instance. The preview identifies those memberships; explicitly resume access only when that new Group should share the source. Import does not change the source Group, copy live lifecycle/receiver authority, or make the imported Group replace the original direct conversation.

Existing CCCC roles still apply: the first visible Actor in membership order is foreman, and the others are peers. A new native-initiated conversation puts its initiating session first; a human-created conversation places the explicitly selected Foreman first, preserving the relative order of the remaining selections. Duplicate selections add one member. Reusing an existing direct conversation preserves its Actor order and roles, even when the other participant initiates the next message. These roles affect Group coordination permissions, not ownership of the native processes. Neither direct sends nor replies require a foreman relay: participants address each other through the Group. The first direct send establishes the Group automatically, so callers do not have to create or join it manually first.

The private Web sidebar discovers these Groups through its existing global event subscription, including Groups created from native MCP without an open local-session dialog. Select the Group to follow messages, replies and delivery receipts in Messages. This view contains CCCC collaboration traffic, not every exchange from the original native sessions. The Terminals view identifies each external source and its availability; it does not display or control the original terminal screen.

## Ownership and delivery

An external Actor's Web pane identifies its native ownership instead of presenting a fake PTY or mirrored transcript. Its name and Group notes remain editable. Configure model, credentials and runtime behavior in the original application. **Pause CCCC access** disables that Group membership; it does not stop the native process. **Resume CCCC access** enables that same source's membership without resuming a stopped or paused Group. The same separation applies to CLI Actor Start/Stop for native members. Managed restart/new-session operations are rejected for external Actors.

Paused CCCC access, a paused/stopped Group, and an unavailable native receive channel are different states. Resume the access or Group you paused; reload the native integration only when its receive channel is unavailable. Removing the Actor removes its membership in that Group, not its original application or native history.

Codex receives on its existing local app-server. While Codex is in the middle of a turn that can take direct input, CCCC adds the message to that turn, as pressing Enter does in the Codex CLI, allowing Codex to use it during that turn instead of in a separate turn after this one ends. When Codex is idle, compacting its context, or waiting for an approval or for your answer, CCCC uses Codex's official `queue` command as before. This applies only to messages CCCC hands over: your own input keeps each Codex client's Enter/Tab or follow-up setting, and a Codex hook can still decline a message after it is handed over. Pi receives through its native extension and follow-up queue. OpenCode receives through its CLI plugin and existing backend queue. Claude receives through one persistent native Monitor, without model-driven polling or rearming. A transport acceptance confirms native handoff, not that the model executed or replied to the message. Pi requires its exact native user-message event because its public queue API has no admission receipt; the other integrations acknowledge their native queue/Monitor.

Enabling the integration is a one-time configuration step, not a per-conversation setup or Group handshake. New conversations and receive-channel reconnects do not require reinstalling the hooks or MCP server. Native identity, Group membership and model context have separate lifetimes:

- **Resume the exact same Codex thread:** it reconnects the same source and retains its Group memberships, once the original application has loaded it and the integration is available.
- **Create a different Codex thread:** it is a new source, even in the same terminal or directory. The old Actor stays bound to the old thread; it is not deleted, disabled or reassigned automatically. The old thread may still receive CCCC messages while the shared app-server keeps it loaded.
- **OpenCode V2:** each open native session is a separate source. Switching or closing a session withdraws that TUI's receive lease when the session is no longer open in a tab. Reload and exit retire the captured revision; reopening the same session can reuse its Actor memberships after the original owner exits. A second live TUI does not take over the first receiver. New/forked sessions have their own sources and require explicit Group admission. The shared backend and native history remain owned by OpenCode.
- **Start a new Pi conversation or reload within the same native host:** the source and Group memberships remain, while the captured lifecycle revision changes. An old extension cannot act for the replacement context. Exiting retires the source. A replacement Pi process is a new source: when it restores the old host's last recorded conversation, for example with `pi --session <id>`, with the same provider configuration, its receive integration reconnects the previous Group memberships under the recovery rules above. Any other replacement requires explicit admission as a new Actor.
- **Clear Claude's conversation within the same native host:** the source and Group memberships remain; a lifecycle callback with a new native conversation ID updates the revision. Clearing context does not leave those Groups. A replacement Claude host is a new source: when it resumes the old host's last recorded conversation with the same provider configuration after that host exited, its receive integration reconnects the previous Group memberships under the recovery rules above. Any other replacement requires explicit admission as a new Actor.

The **Connect existing sessions** directory also shows registered live sessions whose receive channel is not ready; they cannot be selected for admission until ready. With an existing target Group, each source shows whether it is already a member or has paused access. Choose **Resume CCCC access** explicitly to restore a paused member; **Connect to Group** only adds selected new members. Group pause/stop remains independent.

To collaborate from a new source, create a conversation or explicitly join an existing Group through **Connect existing sessions**. Discovery and restoring native history do not join a new identity to old Groups. If you are switching to an unrelated task, explicitly pause CCCC access or leave the old memberships instead of assuming a new or cleared native conversation ends them.

### Resume a native session locally

In the private Web, the administrator can open a native Actor's details to see **Native session recovery**. It separates native receive/lifecycle status from **Pause CCCC access** and Group pause/stop. An unavailable receiver does not prove the native process exited. If the original application is still open, check it and reload its CCCC integration instead of launching a duplicate instance.

Online recovery details default to collapsed. The pane reads on visibility and relevant state changes, not a repeating timer; expanding details does not make another request. Use its single **Refresh status** action for later changes. A recorded End or integration disable remains the reason even after its host exits.

For a live Codex source, **Refresh status** explicitly rechecks the existing receive channel. Claude, Pi and OpenCode refresh read the current recovery record. Neither starts/resumes a session or replays failed/ambiguous deliveries. An inconclusive Codex check keeps a concise failure message alongside the freshly validated recovery record, without another GET or automatic retry. Lost access, a changed binding or an unreadable recovery record clears private details. Ended, exited and disabled identity is not revived by this button.

Restoring readiness can let previously authorized pending work continue through the normal delivery worker, including in other Groups sharing that source. The refresh operation itself does not enqueue or replay messages; pause membership access or the Group first if pending work should remain paused.

The pane offers **Copy directory** and **Copy command**, using the last working directory and native ID registered by CCCC. The snippet includes this CCCC instance and any captured provider configuration directory. Its shell label comes from the CCCC host: Bash/zsh on Linux/macOS, PowerShell on native Windows. Run the command on the original machine and configuration, not necessarily the device displaying the Web page. The Web only displays/copies it; it does not execute it. Recovery details are not available to Group-scoped Web users or Exhibit viewers.

Run the original runtime in its original working directory, using the same provider configuration directory (`CODEX_HOME`, `CLAUDE_CONFIG_DIR` or `PI_CODING_AGENT_DIR`) and CCCC instance (`CCCC_HOME`). To restore a known native conversation:

```sh
codex resume '<native-thread-id>'
claude --resume '<native-conversation-id>'
pi --session '<native-conversation-id>'
opencode --session '<native-session-id>'
```

If you do not know the native ID, use `codex resume`, `claude --resume` `pi --resume` or OpenCode's session picker to choose from the original application's session picker. A Claude or Pi CCCC `source_id` is a host fingerprint, **not** a conversation ID; never pass it to `--resume`.

CCCC retains its last lifecycle metadata after an End or host exit, without retaining live tool/receiver authority or copying provider history. Existing records from before this feature may lack recovery information: the pane reports unknown state/directory, keeps a known Codex thread ID, and otherwise uses the native session picker rather than guessing. A fresh native lifecycle callback captures the current information; unreadable metadata is a read error, not evidence of exit. This guidance cannot guarantee the provider still has the conversation or configuration directory.

These are manual recovery commands, not availability checks: restoring a session can resume queued work and use the model. Resuming the same Codex thread can reconnect its existing memberships. Claude and Pi in a replacement host reconnect their previous memberships when they resume the old host's last recorded conversation with the same provider configuration after that host exited. A different conversation, a still-open original host or another provider configuration needs explicit admission as a new Actor. You do not need to reinstall hooks for any of these cases.

When a replacement Claude or Pi host does not reconnect this way, use **Connect existing sessions** in the sidebar, choose the original **Group**, select the reconnected source, and choose **Connect to Group**. This adds a new Actor without taking over the old Actor, role or pending work. Resume paused membership access and paused/stopped Groups separately when you intend to continue delivery. If integration setup itself was disabled, re-enable it and reload native configuration first; merely resuming provider history does not reinstall the integration.

### Delivery across reconnects

Each Group message is handed off separately in order, with its own receiving Group/Actor/role/scope context even after bootstrap. Old handoff confirmations cannot confirm newer work. Across different Groups, the native runtime owns execution order.

Known rejection is recorded as failed; an uncertain submission is recorded as ambiguous and is not automatically repeated. One such outcome releases its slot so later messages can proceed. A missing Monitor acknowledgement is bounded to 30 seconds. Ordinary delivery to an unavailable source stays pending without a durable claim until its channel is ready for handoff, including across a daemon restart; other sources remain independent. If access or the Group is paused **before native submission**, a claimed attempt carries `retryable=true` and resumes as pending work. Already accepted or uncertain handoffs never replay automatically. Older failed receipts without explicit unsubmitted evidence also require a deliberate retry. After an abrupt daemon exit, unsettled claims at the handoff boundary still become ambiguous and require review rather than automatic replay. Normal Group delivery controls provide an explicit retry where appropriate.

Failure to start the native queue process is known not to have submitted input and remains retryable; failure after it started can still be ambiguous. Codex's command-line limits apply to the complete rendered delivery, including Group context and bootstrap. An oversized or otherwise unrepresentable payload fails explicitly before claiming it: shorten the message or use an attachment. CCCC does not truncate or silently split it. The official queue takes message text in process arguments, which can be visible to other processes with the host OS's command-line inspection permissions.

Pending backlogs larger than an in-memory queue or startup query page continue in bounded batches as delivery capacity becomes available. They do not require another message, repeated status refreshes or a manual Actor Start to reach the remaining items. Paused/disabled memberships and paused/stopped Groups remain fenced; enabling an imported native Actor restores only that membership, never starts the original application, and does not resume a paused/stopped Group.

Receiver presence is ephemeral and reconnects after a daemon restart. Current native identity comes from CCCC-owned lifecycle metadata, including callbacks while the daemon is down. No native conversation database or transcript is scanned. The global directory is restricted to the private Web administrator and is unavailable in Exhibit or through remote model connectors.

## Disconnect the integration

```sh
cccc sessions disable --runtime codex
cccc sessions disable --runtime claude
cccc sessions disable --runtime pi
cccc sessions disable --runtime opencode
```

Disabling first fences native access for that runtime in this `CCCC_HOME`, including callbacks still loaded in existing applications and previously unseen sources. Provider cleanup failure does not reopen that access. Re-enable explicitly and reload native configuration when you want to reconnect; a fresh lifecycle callback is required, and configuration enablement alone does not revive retained identities.

- **Codex:** removes CCCC's own hooks and restores ordinary CCCC MCP configuration.
- **Pi:** unregisters this installation's local package and removes its unmodified `cccc` MCP entry. Run `/reload` to unload them in an existing Pi process. Modified MCP entries are preserved.
- **Claude:** disables the local plugin and removes the unmodified native user MCP entry only when its executable, arguments and `CCCC_HOME` match this installation. It does not install an ordinary global MCP. Entries edited by the user or belonging to another installation remain unchanged, with a notice. An unreadable configuration or ambiguous configuration path is an explicit cleanup error, not proof that the entry is absent.

Claude cleanup reads the effective user configuration without starting MCP servers, then uses the official remove command and checks the resulting file. That CLI has no atomic compare-and-remove operation; avoid concurrent edits to the same user MCP entry during cleanup. Native processes and history are retained. Disabling does not erase Group ledgers or unsent messages.

OpenCode disable closes native authority before removing its unchanged generated plugin files. User modifications are preserved and reported for manual resolution. The plugin's temporary MCP transform is removed on native reload; any pre-existing user MCP configuration remains intact.
