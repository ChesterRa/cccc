# Notebook Binding + NotebookLM (Web)

This guide covers the user-facing Web flow for connecting NotebookLM and choosing which notebooks CCCC should use.

The Web UI is intentionally minimal:

1. connect Google
2. choose the `Work Notebook`
3. choose the `Memory Notebook`

Actual NotebookLM operations such as query, ingest, source management, artifacts, and job handling are handled by agents through MCP / CLI surfaces, not by the normal user settings page.

## 1. Enable Real Provider Path

Start CCCC with the real NotebookLM adapter enabled:

```bash
export CCCC_NOTEBOOKLM_REAL=1
cccc
```

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
- The Docker image includes the minimal Chromium shared libraries needed for the projected sign-in browser. Playwright / Chromium binaries themselves are still installed lazily on first use.

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

## 9. Quick Rollback

If NotebookLM is unstable in your environment:

```bash
unset CCCC_NOTEBOOKLM_REAL
cccc daemon restart
```

## 10. Repo Space Sync Notes

When a group has a local scope attached, CCCC still uses repo-local `space/` as the work-lane resource source of truth:

`<scope_root>/space/`

Relevant metadata files remain:

- `<scope_root>/space/.space-index.json`
- `<scope_root>/space/.space-sync-state.json`
- `<scope_root>/space/.space-status.json`
- `<scope_root>/space/.sync/remote-sources/*.json`
- `<scope_root>/space/artifacts/notebooklm/...`

These implementation details matter for agent/developer workflows, but they are not part of the normal user-facing binding flow.
