# Notebook Binding + NotebookLM (Web)

This guide covers the user-facing Web flow for connecting NotebookLM and choosing which notebooks CCCC should use.

The Web UI is intentionally minimal:

1. connect Google
2. choose the `Work Notebook`
3. choose the `Memory Notebook`

Actual NotebookLM operations such as query, ingest, source management, artifacts, and job handling are handled by agents through MCP / CLI surfaces, not by the normal user settings page.

## 1. Provider Availability

The Rust build connects directly to NotebookLM for account health, notebook
listing/creation, text-source ingest, source listing/rename/delete, and grounded
queries. It does not launch a Python sidecar. Pass `--provider local` only when
you explicitly want the degraded local fallback.

NotebookLM still has no public consumer API. CCCC therefore isolates Google's
private RPC catalog and positional response adapters in `cccc-notebooklm` and
fails with `provider_schema_drift` when the wire contract changes. Artifact
generation/download and repo-wide sync are not enabled yet and fail explicitly;
they never report a fabricated remote success.

If you expose Web outside localhost, first create an **Admin Access Token** in **Settings > Web Access** and keep the service behind a network boundary until that token exists.

## 2. Open Notebook Settings

1. Open a target group in Web.
2. Open **Settings**.
3. Open the **Notebook** tab.

## 3. Connect Google

In **Google Account**:

1. Click **Connect Google**.
2. Complete sign-in in the interactive browser view shown inside CCCC Web.
3. After the page returns to `notebooklm.google.com`, CCCC captures the browser
   storage state, closes the sign-in browser, validates the account, and refreshes
   the notebook selector.

Notes:

- If a valid credential is already stored, reconnect may complete without a full browser login.
- Authentication completion is owned by the Rust server task. Closing or refreshing
  the settings page does not submit credentials and does not interrupt the flow.
- Each sign-in uses a one-time browser profile. Success, failure, cancellation,
  timeout, and server shutdown all close the browser and remove that profile.
- **Reconnect** validates the saved session first. A forced reconnect skips saved
  cookies, while **Disconnect** removes both the credential and legacy browser profile.
- Google may rotate session cookies during normal API calls. CCCC persists those
  rotations only when the credential came from the CCCC store; credentials supplied
  through `CCCC_NOTEBOOKLM_AUTH_JSON` remain read-only.
- The default Web page does not expose manual credential editing anymore.
- The Web flow uses a projected sign-in browser so Docker / remote deployments do not need a local desktop browser on the daemon host.
- The current Rust projected surface uses Chromium's isolated headless mode and does
  not attach to the host desktop display.
- The Rust surface launches a discovered system Chromium/Chrome binary; it does
  not depend on a Python Playwright sidecar or a persistent desktop profile.

This is browser-session authorization, not Google OAuth. The consumer Gemini
Notebook service does not currently publish an OAuth scope or supported public
API for these operations. CCCC deliberately does not request or retain a
Google-account-wide master token.

## 4. Bind the Work Notebook

In **Work Notebook**:

1. Choose an existing notebook from the selector, or
2. Click **Create and bind new**.

Use `Work Notebook` for shared project knowledge and working materials.

Expected result:

- Work binding becomes `Bound`.
- The current notebook title/id updates immediately.

## 5. Bind the Memory Notebook

In **Memory Notebook**:

1. Choose an existing notebook from the selector, or
2. Click **Create and bind new**.

Use `Memory Notebook` for finalized memory recall.

Expected result:

- Memory binding becomes `Bound`.
- The current notebook title/id updates immediately.

## 6. Connection Summary

Use **Connection Summary** only as a lightweight status snapshot:

1. Google connected or not
2. Work notebook bound or not
3. Memory notebook bound or not
4. a short warning message if something is degraded

The summary is intentionally human-oriented and does not expose internal queue/job/runtime details.

## 7. What the Web Page No Longer Does

The normal user-facing Web settings page no longer exposes these agent/developer operations:

1. Notebook query
2. ingest submission
3. source management
4. artifact generation/download
5. job queue operations
6. manual credential write/clear
7. provider health check

That is by design.

## 8. Agent-Side Usage Still Exists

Group Space controls still exist through agent-facing surfaces:

1. MCP tools
2. CLI
3. prompt/help-guided agent workflows

The Web page is only for account connection and notebook binding. Agent-facing
NotebookLM operations use the same Rust daemon adapter as the Web routes.

## 9. Repo Space Sync Notes

When a group has a local scope attached, CCCC still uses repo-local `space/` as the work-lane resource source of truth:

`<scope_root>/space/`

Relevant metadata files remain:

- `<scope_root>/space/.space-index.json`
- `<scope_root>/space/.space-sync-state.json`
- `<scope_root>/space/.space-status.json`
- `<scope_root>/space/.sync/remote-sources/*.json`
- `<scope_root>/space/artifacts/notebooklm/...`

These implementation details matter for agent/developer workflows, but they are not part of the normal user-facing binding flow.
