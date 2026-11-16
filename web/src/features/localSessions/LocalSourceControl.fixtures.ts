import { vi } from "vite-plus/test";
import type { LocalSessionSource } from "../../types";
import type { LocalSessionDirectory, SourceState } from "../../services/api/localSessionControls";

/** Synthetic backend boundary only; no UI stores or management hooks are replaced. */
export function sourceControlFixture(
  source: LocalSessionSource,
  publish: (directory: LocalSessionDirectory) => void,
) {
  const remote: LocalSessionSource = {
    ...source,
    source_id: "remote:qa:source",
    title: "Remote QA source",
    origin: "remote:QA laptop",
  };
  const state: SourceState = {
    control: {
      supported: true,
      management_revision: "0",
      access_blocked: false,
      pause_complete: true,
    },
    diagnostic: { state: "unknown", checked_at: null },
  };
  let connected = false;
  let includeRemote = false;
  let mode = "hold-fail";
  let finish!: (response: Response) => void;
  let serial = 0;
  const directory = (): LocalSessionDirectory => ({
    sources: [
      ...(state.control.access_blocked ? [] : [{ ...source, connected }]),
      ...(includeRemote ? [remote] : []),
    ],
    removed_sources: state.control.access_blocked ? [{ ...source, connected: false }] : [],
    capabilities: { source_control: true, source_diagnostics: true },
    source_states: {
      [source.source_id]: state,
      ...(includeRemote
        ? {
            [remote.source_id]: {
              control: {
                supported: false,
                management_revision: "0",
                access_blocked: false,
                pause_complete: true,
                reason_code: "remote_source_control_unsupported",
              },
              diagnostic: { state: "unknown" as const, checked_at: null },
            },
          }
        : {}),
    },
  });
  const refresh = () => publish(JSON.parse(JSON.stringify(directory())) as LocalSessionDirectory);
  const result = () => ({
    source_id: source.source_id,
    ...state.control,
    status: state.control.access_blocked
      ? state.control.pause_complete
        ? "removed"
        : "partial"
      : "active",
  });
  const request = vi.fn(async (url: unknown, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body));
    if (String(url).endsWith("/recheck")) {
      state.diagnostic = { state: "thread_not_loaded", checked_at: new Date().toISOString() };
      refresh();
      return Response.json({
        ok: true,
        result: {
          source_id: source.source_id,
          diagnostic: state.diagnostic,
          check_status: "cached",
          retry_after_ms: 15000,
        },
      });
    }
    if (!String(url).endsWith("/control")) throw new Error("Unexpected backend route");
    if (mode === "hold-fail")
      return new Promise<Response>((resolve) => {
        finish = resolve;
      });
    if (mode === "restore-conflict") {
      state.control.management_revision = "management-conflict";
      refresh();
      return Response.json(
        {
          ok: false,
          error: {
            code: "source_control_conflict",
            message: "changed",
            details: { current: result() },
          },
        },
        { status: 409 },
      );
    }
    if (body.expected_revision !== state.control.management_revision)
      return Response.json(
        { ok: false, error: { code: "source_control_conflict", message: "stale" } },
        { status: 409 },
      );
    state.control = {
      ...state.control,
      management_revision: `management-${++serial}`,
      access_blocked: body.action === "remove",
      pause_complete: mode !== "partial-error",
    };
    refresh();
    return mode === "partial-error"
      ? Response.json(
          { ok: false, error: { code: "pause_incomplete", message: "blocked but partial" } },
          { status: 503 },
        )
      : Response.json({ ok: true, result: result() });
  });
  return {
    snapshot: () => JSON.parse(JSON.stringify(directory())) as LocalSessionDirectory,
    request,
    refresh,
    state,
    remote,
    setMode: (value: string) => {
      mode = value;
    },
    ready: () => {
      connected = true;
      state.diagnostic = { state: "ready", checked_at: new Date().toISOString() };
      refresh();
    },
    addRemote: () => {
      includeRemote = true;
      refresh();
    },
    failPending: () =>
      finish(
        Response.json(
          { ok: false, error: { code: "unavailable", message: "no commit" } },
          { status: 503 },
        ),
      ),
  };
}

export function deferDirectoryRead(fetch: {
  mockReturnValueOnce: (value: Promise<unknown>) => unknown;
}) {
  let resolve!: (value: unknown) => void;
  fetch.mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  return (directory: LocalSessionDirectory) => resolve({ ok: true, result: directory });
}
