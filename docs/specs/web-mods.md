# Web Mods Specification (draft)

Status: draft for review, not implemented. Tracked as T968. Scope chosen for v1: style mods and script mods with extension slots together.

## Goal

A user, or an agent acting on the user's request, can reshape the CCCC Web UI without rebuilding the binary or forking the frontend:

- **Style mods** change how anything looks: colours, fonts, spacing, hiding or moving elements.
- **Script mods** add behaviour through fixed extension slots: a sidebar panel, message actions, composer tools, a settings page, a standalone page.

Two properties are not negotiable:

1. **Changes show at once.** Saving a mod updates every open page without a reload or a rebuild.
2. **A broken mod is always recoverable.** Safe mode skips every mod, and each mod can be switched off on its own.

## Current state (read-only findings)

- Customisation is limited to branding (product name, logo, favicon in `state/web_branding/`) and per-browser LCD preferences (`web/src/utils/uiStyle.ts`).
- The Web bundle is compiled into the binary through `rust_embed` from `$CCCC_WEB_DIST_DIR` (`crates/cccc-web/src/lib.rs:133`).
- The Content-Security-Policy only sets `frame-ancestors` (`crates/cccc-web/src/security_headers.rs`). Injected styles and scripts are not blocked, so the boundary must come from this design.
- Access tokens are admin or group-scoped (`crates/cccc-web/src/auth.rs`, `requires_admin`, `Principal::allows`).
- `/api/v1/events/stream` is a global event stream every page already holds (`web/src/hooks/useGlobalEvents.ts`).
- The frontend carries about 571 distinct `data-*` attributes, and the theme is driven by CSS variables (for example `--lcd-ink`, `--color-text-primary`). Both are natural styling hooks.

## Mod package

A mod is a directory under the instance home:

```
<CCCC_HOME>/web_mods/<mod_id>/
  mod.json        manifest (required)
  style.css       optional
  main.js         optional, ES module
  assets/...      optional static files the mod references
```

`mod.json`:

```json
{
  "id": "wide-sidebar",
  "name": "Wide sidebar",
  "version": "1",
  "api": 1,
  "description": "Widen the group sidebar and hide the New button.",
  "style": "style.css",
  "script": "main.js",
  "slots": ["sidebar.panel"],
  "author": "agent:claude-1"
}
```

- `id` matches the directory name: lowercase letters, digits and dashes, at most 64 characters.
- `api` is the extension API version the mod was written for. A mod for an unknown version is listed but not loaded.
- `slots` declares where a script may render. Rendering into an undeclared slot is refused, so a mod's reach is visible before it runs.
- Size limits: 256 KiB per file and 2 MiB per mod, matching the branding asset limit.

Instance state, outside the mod directories, in `<CCCC_HOME>/state/web_mods.json`:

```json
{
  "v": 1,
  "mods": {
    "wide-sidebar": {
      "enabled": true,
      "script_approved_sha256": "…",
      "order": 10,
      "updated_at": "…"
    }
  }
}
```

## Trust model

A script mod runs in the page with the viewer's access token, so it can do anything that viewer can. The boundary is:

1. **Only an admin installs, enables, edits or removes mods.** Every write route requires `is_admin`.
2. **A script runs only after an admin approves exactly that script.** Approval stores the script's SHA-256. If the file changes, the script stops loading until it is approved again. Style changes need no approval; the mod's enabled switch still applies.
3. **Agents may write mods but may not approve scripts.** An agent can create or edit a mod's files, and its style takes effect at once. A new or changed script waits in "awaiting approval" until the user approves it in the Web UI. An agent can never make code run with the user's token on its own.
4. **Mods are global.** A mod applies to the whole instance; there is no per-group mod. Group-scoped viewers get the same mods. A script receives only what the viewer's own token allows, because every API call it makes goes through the normal authorisation.
5. **Safe mode.** Adding `?safe=1` to the address, or holding Shift while the page loads, skips every mod for that page. The Mods settings page always opens in safe mode, so a mod can never hide its own off switch.

## Loading

1. `index.html` loads as today.
2. Unless in safe mode, the app fetches `GET /api/v1/web_mods/manifest`. It returns the enabled mods in order, each with a content hash per file and whether its script is approved.
3. Styles are inserted as `<link>` elements after the app's own stylesheets, so a mod's rules win at equal specificity. Each `<link>` carries `data-cccc-mod="<id>"`.
4. Approved scripts are imported with `import("/web_mods/<id>/main.js?h=<hash>")`. Each module's default export is a function receiving the extension API (below). One mod's failure is caught and shown on the Mods page; it does not stop the others or the app.
5. `GET /web_mods/<id>/<path>` serves mod files to any authenticated viewer, with `Cache-Control: no-cache` and an `ETag` equal to the content hash.

**Hot reload.** Each listing carries a `revision` that changes with any mod, enabled switch or served file. In the first version a visible page polls the listing once a second and applies changes when the revision moves; pages already share one WebSocket for events, and an extra HTTP stream per tab would compete for the browser's per-origin connection limit. A later version can publish `web_mods.changed` on the shared event socket instead. Pages swap changed style links in place. For a changed approved script, the page calls the old module's cleanup (below), then imports the new one. Rapid edits are debounced to one update every 300 ms.

## Extension API (version 1)

```ts
export default function activate(cccc: CcccModApi): void | (() => void);

interface CcccModApi {
  apiVersion: 1;
  mod: { id: string; version: string };
  slots: {
    // Each returns a disposer. Rendering into an undeclared slot throws.
    render(slot: SlotId, mount: (host: HTMLElement, context: SlotContext) => void | (() => void)): () => void;
  };
  ui: {
    toast(text: string): void;
    openPage(pageId: string): void;
  };
  data: {
    currentGroup(): GroupSummary | null;
    onGroupChange(listener: (group: GroupSummary | null) => void): () => void;
    onMessage(listener: (event: LedgerMessageSummary) => void): () => void; // current group only
  };
  actions: {
    sendMessage(text: string, options?: { to?: string[]; mode?: "send" | "mail" | "request_reply" }): Promise<void>;
  };
  storage: {
    // Per mod and per browser; never shared with other mods.
    get(key: string): string | null;
    set(key: string, value: string): void;
  };
}
```

Version 1 slots:

| Slot id | Where | Context passed |
|---|---|---|
| `sidebar.panel` | Bottom of the expanded group sidebar | current group |
| `message.actions` | The action row on each message | the message's id, sender, text, group |
| `composer.tools` | The composer toolbar, beside attachments | current group, a function to insert text |
| `settings.page` | A tab in Settings named by the mod | none |
| `page` | A standalone page, opened with `ui.openPage` | none |

Rules:

- A slot host is a plain element owned by the app. A mod renders inside it with any library it bundles, or with plain DOM. The app removes the host when the slot unmounts and calls the disposer the mount function returned.
- Mods do not receive app internals (stores, React, API clients). Everything goes through `CcccModApi`, so the app can change internally without breaking mods. The API version changes only when this surface changes incompatibly.
- `actions.sendMessage` uses the viewer's token and the same route as the composer. It is subject to the same permissions and appears in the ledger as sent by the viewer.

## Agent workflow

1. The user asks in a group, for example "widen the sidebar and add a panel listing today's tasks".
2. The agent writes the mod through an MCP tool `cccc_web_mod` with actions `list | read | write | enable | disable | remove`. Writes go to `web_mods/` only, enforce the manifest and size rules, and record `author`. The agent never gets a general path into `CCCC_HOME`.
3. Style changes show at once. A script change makes the Mods page and the composer show "Wide sidebar has a new script awaiting approval", with a diff view and Approve / Reject.
4. After approval the page hot-loads the script. The user can switch the mod off or roll it back to the previously approved version, which is kept as `main.js.approved`.

A CLI mirrors the tool: `cccc mod list|enable|disable|approve|remove|path`. `approve` is interactive and admin-only.

## Web UI: Settings → Mods

- A list of mods with an enabled switch, name, author, slots, and status: running / awaiting approval / failed with the error.
- Approve a pending script with a diff against the last approved version.
- "Open in safe mode" and "Disable all".
- Roll back a script to its last approved version.
- The page itself always loads with mods skipped.

## Sharing, import and the website gallery

Mods can be exported, shared and installed on other instances, and the official website (the `cccc-hero` project) shows a gallery where visitors browse mods and switch between them.

### Export format

`<id>-<version>.ccccmod` is a zip archive:

```
bundle.json      format version, mod id and version, sha256 of every file, exported_at, exporter instance name
mod.json
style.css        if present
main.js          if present
assets/...       if present
preview/...      optional screenshots for galleries (PNG or WebP, at most 4, 1 MiB each)
```

- `cccc mod export <id>` and an Export button on Settings → Mods produce the file. Only approved script content is exported; an unapproved edit is not.
- Archive limits match the package limits; entries with absolute paths or `..` are rejected before anything is unpacked.

### Import

- Sources: a `.ccccmod` file (upload or `cccc mod import <path>`), or an HTTPS URL to one (`cccc mod import <url>`, or Import from URL on the Mods page).
- Every file is checked against `bundle.json` hashes before installation.
- An imported mod installs **disabled**. Its style can be enabled at once; its script follows the normal approval rule. The approval view shows the source (file name or URL), the declared author and the script, since this is code from outside the instance.
- Importing a mod whose id already exists offers to replace it (keeping the old version for rollback) or install under a new id.

### Website gallery (cccc-hero)

- **Registry:** the website publishes `mods/index.json`: for each mod its id, name, description, author, version, `api` version, whether it has a script, the slots it uses, preview image URLs, the download URL of its `.ccccmod`, and its sha256.
- **Gallery page:** cards with preview, description, author and a script badge ("adds features" vs "changes appearance").
- **Switching:** visitors switch between mods on a preview of the CCCC UI (see the open decision below).
- **Install:** an Install button opens the visitor's own CCCC at `http://127.0.0.1:8848/#/settings/mods?import=<url>` (or the address they set). The local Mods page shows the import dialog with the source, hash and script; nothing is installed or run without the admin confirming there.
- **Publishing:** for v1, mods enter the gallery through a curated submission (a pull request adding the `.ccccmod` and registry entry to `cccc-hero`). Self-service upload through the website account is a later step.

### Open decision: how visitors switch mods on the website

- **A. Preview images.** Each mod ships screenshots; switching swaps images. Simple and safe, but visitors see pictures, not the real effect.
- **B. Live demo shell.** The CCCC Web UI is built once as a static demo with fixture data (no daemon) and embedded on the website in a sandboxed iframe; switching applies the selected mod's style (and, for curated mods, its script) to the demo live. The most convincing, but it adds a demo build of the Web UI that must be kept in step with releases.

## Implementation outline

Daemon and web server (Rust):

- `cccc-core`: mod package validation, state file, approval hashes, watcher → `web_mods.changed` event.
- `cccc-web`: manifest route, file route, admin write routes (enable, disable, approve, remove, rollback), auth via the existing middleware.
- `cccc-mcp`: `cccc_web_mod` tool, with writes limited to `web_mods/`.
- `cccc-cli`: `cccc mod …`.

Frontend:

- `web/src/features/mods/`: loader, hot reload, error isolation, safe mode, the `CcccModApi` implementation, and the slot host component `<ModSlot id="…"/>` placed at each slot.
- Settings → Mods page.
- English, Chinese and Japanese strings.

## Acceptance criteria

1. A style mod written to `web_mods/` changes an open page within one second without a reload; disabling it reverts the page.
2. A script mod is not loaded until approved. Editing the script after approval stops it loading until it is approved again.
3. An agent using `cccc_web_mod` can write and enable a mod but cannot approve its script; the tool has no approve action and the approve route rejects non-admin tokens.
4. A script that throws during activation, or renders into an undeclared slot, is reported on the Mods page while the app and other mods keep working.
5. `?safe=1` loads no mod files at all (no requests to `/web_mods/`), and the Mods page always does the same.
6. A group-scoped viewer's mod script gets permission errors for groups its token cannot access.
7. Mod files over the size limit, or paths escaping the mod directory, are rejected.
8. Hot-reloading a script calls the previous module's disposer before the new module activates, so no slot renders twice.
9. An exported `.ccccmod` imports on another instance with identical file hashes; a tampered file or an archive path escaping the mod directory is rejected; an imported mod starts disabled and its script requires approval.
10. The website Install button opens the local import dialog and installs nothing until the admin confirms.

## Delivery in phases (all within v1)

1. **Foundation:** package format, state, manifest and file routes, loader with safe mode, style mods with hot reload. This makes style mods usable end to end.
2. **Scripts:** approval flow, extension API, the five slots, error isolation, Settings → Mods.
3. **Agent tooling:** `cccc_web_mod` MCP tool, CLI, rollback.
4. **Sharing:** export and import (file and URL), import dialog, install deep link.
5. **Website gallery (cccc-hero):** registry format, gallery page, mod switching per the chosen preview option, curated submission.

## Open questions

1. Resolved (2026-11-24): mods are global. A mod applies to the whole instance, never to one group.
2. Should non-admin viewers be able to turn a mod off for themselves in their browser? The proposal is yes, through per-browser storage, without affecting others.
3. Resolved (2026-11-24): mods are shareable. See "Sharing, import and the website gallery". Open: how the website previews a mod (A or B above).
