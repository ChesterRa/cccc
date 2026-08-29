# Notebook Binding + NotebookLM (Web)

This guide covers the user-facing Web flow for connecting NotebookLM and choosing which notebooks CCCC should use.

The Web UI is intentionally minimal:

1. connect Google
2. choose the `Work Notebook`
3. choose the `Memory Notebook`

Actual NotebookLM operations such as query, ingest, source management, artifacts, and job handling are handled by agents through MCP / CLI surfaces, not by the normal user settings page.

## 1. Provider Activation

Connecting Google in CCCC Web stores the provider credential and activates the
real NotebookLM path. CCCC does not require an environment toggle.

If you expose Web outside localhost, first create an **Admin Access Token** in **Settings > Web Access** and keep the service behind a network boundary until that token exists.

## 2. Open Notebook Settings

1. Open a target group in Web.
2. Open **Settings**.
3. Open the **Notebook** tab.

## 3. Connect Google

In **Google Account**:

1. Click **Connect Google**.
2. Complete sign-in in the interactive browser view shown inside CCCC Web.
3. Wait until the account status becomes connected.

Notes:

- If a valid credential is already stored, reconnect may complete without a full browser login.
- The default Web page does not expose manual credential editing anymore.
- The Web flow uses a projected sign-in browser so Docker / remote deployments do not need a local desktop browser on the daemon host.
- The projected sign-in browser now runs in headed mode for better Google compatibility. In server/container environments without a native display, CCCC uses `Xvfb` automatically.
- The Docker image installs system Chromium and its display dependencies during
  the image build; projected sign-in does not perform a lazy browser download.

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

NotebookLM usage still exists through agent-facing surfaces:

1. MCP tools
2. CLI
3. prompt/help-guided agent workflows

The Web page is now only for account connection and notebook binding.

## 9. Disconnect

Use **Disconnect Google** in the Notebook settings when this machine should no
longer use the stored NotebookLM credential.

## 10. Explicit source ingestion and legacy sync state

For 0.4.36, files are added explicitly from the attached project scope. For
example, a caller can ingest `<scope_root>/space/spec.md`; CCCC validates the
resolved path and uploads it without turning the entire directory into an
automatic mirror.

`<scope_root>/space/`

Homes upgraded from Python 0.4.35 may still contain these legacy status files:

- `<scope_root>/space/.space-index.json`
- `<scope_root>/space/.space-sync-state.json`
- `<scope_root>/space/.space-status.json`
- `<scope_root>/space/.sync/remote-sources/*.json`
- `<scope_root>/space/artifacts/notebooklm/...`

The native Rust adapter follows the `notebooklm-py` v0.8.1 protocol baseline for
the current personal-app host, source requests, artifact status values, and
audio output format. It supports attached-scope local files, pasted text, Web
URL, YouTube, and Google Drive Docs/Slides/Sheets ingestion.

Each explicit ingest is recorded before the provider write. A normal request
makes one provider attempt and settles the job; failed jobs are retried only by
an explicit `space jobs retry`, not by a hidden background loop. A job left
`running` after a process interruption or an ambiguous provider response
represents an uncertain provider result. Repeating the same ingest is deduped
to that durable job, and direct retry is blocked until the user inspects and
reconciles the remote notebook or cancels the job. Retrying a terminal old job
is also refused if the Group's current work binding no longer matches the
notebook saved on that job.

Automatic work- and memory-lane mirroring is retired in 0.4.36. Rust reads the
same canonical metadata so existing status survives an upgrade, but it never
resumes the old remote mutation loop. The CLI, Web API, and MCP surface no
longer advertise a sync action.

These implementation details matter for agent/developer workflows, but they are not part of the normal user-facing binding flow.
