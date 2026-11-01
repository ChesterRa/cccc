// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type {
  ProcessRecoveryDiagnostic,
  ProcessRecoveryOwner,
  ProcessRecoveryRecord,
} from "../../services/api/processRecovery";
import { ProcessRecoveryDialog } from "./ProcessRecoveryDialog";

const api = vi.hoisted(() => ({ fetch: vi.fn(), retry: vi.fn(), accept: vi.fn() }));

vi.mock("react-i18next", () => {
  const t = (key: string, options?: Record<string, unknown>) =>
    options ? `${key}:${JSON.stringify(options)}` : key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../../services/api/processRecovery", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api/processRecovery")>()),
  fetchProcessRecovery: api.fetch,
  retryProcessRecovery: api.retry,
  acceptProcessRecovery: api.accept,
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

let root: Root | null = null;

beforeEach(() => {
  api.fetch.mockReset();
  api.retry.mockReset();
  api.accept.mockReset();
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  document.body.innerHTML = "";
});

function record(id: string, overrides: Partial<ProcessRecoveryRecord> = {}): ProcessRecoveryRecord {
  return {
    id,
    version: "v1",
    kind: "process_group",
    created_at_unix_seconds: null,
    origin: "same_origin",
    reason: "cleanup_unconfirmed",
    retryable: true,
    acceptance_supported: true,
    ...overrides,
  };
}

function listed(
  owner: ProcessRecoveryOwner,
  records: ProcessRecoveryRecord[],
  diagnostics: ProcessRecoveryDiagnostic[] = [],
) {
  return { ok: true, result: { owner, records, diagnostics } };
}

/** Serve each owner's list from a queue; the last entry repeats. */
function serve(lists: Partial<Record<ProcessRecoveryOwner, unknown[]>>) {
  api.fetch.mockImplementation(async (owner: ProcessRecoveryOwner) => {
    const queue = lists[owner] ?? [listed(owner, [])];
    return queue.length > 1 ? queue.shift() : queue[0];
  });
}

async function mount(owners: ProcessRecoveryOwner[]) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  const onClose = vi.fn();
  await act(async () =>
    root!.render(<ProcessRecoveryDialog isOpen owners={owners} isDark={false} onClose={onClose} />),
  );
  return onClose;
}

function button(text: string): HTMLButtonElement {
  const found = Array.from(document.body.querySelectorAll("button")).find(
    (element) => element.textContent === text,
  );
  if (!found) throw new Error(`no button ${text}`);
  return found;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function rows() {
  return document.body.querySelectorAll("[data-process-recovery-record]");
}

function text() {
  return document.body.textContent ?? "";
}

async function click(label: string) {
  await act(async () => button(label).click());
}

describe("ProcessRecoveryDialog", () => {
  it("reads only the owners it was opened for, each in its own section", async () => {
    serve({ web: [listed("web", [record("a"), record("b", { kind: "launch_intent" })])] });
    await mount(["web"]);
    expect(api.fetch).toHaveBeenCalledTimes(1);
    expect(api.fetch.mock.calls[0][0]).toBe("web");
    expect(document.body.querySelector('[data-process-recovery-owner="daemon"]')).toBeNull();
    expect(text()).toContain("processRecovery.ownerSection.web · 2");
    expect(text()).toContain("processRecovery.kinds.launch_intent");
    expect(rows()).toHaveLength(2);
  });

  it("shows the empty state when no shown owner has anything to review", async () => {
    serve({ daemon: [listed("daemon", [])], web: [listed("web", [])] });
    await mount(["daemon", "web"]);
    expect(text()).toContain("processRecovery.empty");
    expect(text()).not.toContain("processRecovery.intro");
  });

  it("drops a cleared row and refreshes a row that is still pending", async () => {
    serve({ daemon: [listed("daemon", [record("a"), record("b")])] });
    api.retry.mockResolvedValueOnce({
      ok: true,
      result: { owner: "daemon", result: "cleared", record: null, diagnostics: [] },
    });
    await mount(["daemon"]);
    await act(async () => (rows()[0].querySelector("button") as HTMLButtonElement).click());
    expect(rows()).toHaveLength(1);
    expect(text()).toContain("processRecovery.cleared");

    api.retry.mockResolvedValueOnce({
      ok: true,
      result: {
        owner: "daemon",
        result: "still_pending",
        record: record("b", { version: "v2", reason: "signal_failed" }),
        diagnostics: [],
      },
    });
    await click("processRecovery.retry");
    expect(rows()).toHaveLength(1);
    expect(text()).toContain("processRecovery.reasons.signal_failed");
    expect(text()).toContain("processRecovery.stillPending");
    // A retry sends the version the user saw, and nothing reads the list again.
    expect(api.retry.mock.calls[1][1]).toEqual(expect.objectContaining({ id: "b", version: "v1" }));
    expect(api.fetch).toHaveBeenCalledTimes(1);
  });

  it("asks before accepting and moves focus to the confirmation", async () => {
    serve({ daemon: [listed("daemon", [record("a")]), listed("daemon", [])] });
    api.accept.mockResolvedValue({ ok: true, result: { owner: "daemon", accepted: true } });
    await mount(["daemon"]);
    await click("processRecovery.accept");
    expect(api.accept).not.toHaveBeenCalled();
    expect(text()).toContain("processRecovery.acceptRisk");
    expect(text()).not.toContain("processRecovery.acceptRiskOpaque");
    expect(document.activeElement).toBe(button("processRecovery.confirmAccept"));
    await click("processRecovery.confirmAccept");
    expect(api.accept).toHaveBeenCalledOnce();
    expect(text()).toContain("processRecovery.empty");
  });

  it("returns focus to Accept when the confirmation is cancelled", async () => {
    serve({ daemon: [listed("daemon", [record("a")])] });
    await mount(["daemon"]);
    await click("processRecovery.accept");
    await click("common:cancel");
    await act(async () => new Promise<number>((resolve) => requestAnimationFrame(resolve)));
    expect(document.activeElement).toBe(button("processRecovery.accept"));
  });

  it("names the unread contents of opaque evidence before accepting it", async () => {
    serve({ web: [listed("web", [record("a", { kind: "opaque_evidence" })])] });
    await mount(["web"]);
    await click("processRecovery.accept");
    expect(text()).toContain("processRecovery.acceptRiskOpaque");
    expect(text()).toContain("processRecovery.kinds.opaque_evidence");
  });

  it("reads the list again after a conflict instead of replaying the request", async () => {
    serve({
      daemon: [listed("daemon", [record("a")]), listed("daemon", [record("a", { version: "v2" })])],
    });
    api.accept.mockResolvedValue({
      ok: false,
      error: { code: "process_recovery_changed", message: "changed" },
    });
    await mount(["daemon"]);
    await click("processRecovery.accept");
    await click("processRecovery.confirmAccept");
    expect(api.accept).toHaveBeenCalledOnce();
    expect(api.fetch).toHaveBeenCalledTimes(2);
    expect(text()).toContain("processRecovery.changed");
    expect(rows()).toHaveLength(1);
  });

  it("treats a lost reply as unconfirmed and lists again", async () => {
    serve({ daemon: [listed("daemon", [record("a")])] });
    api.retry.mockResolvedValue({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "aborted" },
    });
    await mount(["daemon"]);
    await click("processRecovery.retry");
    expect(api.retry).toHaveBeenCalledOnce();
    expect(api.fetch).toHaveBeenCalledTimes(2);
    expect(text()).toContain("processRecovery.uncertain");
  });

  it("does not count a success without a matching receipt as handled", async () => {
    serve({ daemon: [listed("daemon", [record("a")])] });
    api.accept.mockResolvedValueOnce({ ok: true, result: {} });
    api.retry.mockResolvedValueOnce({ ok: true, result: { owner: "web", result: "cleared" } });
    await mount(["daemon"]);
    await click("processRecovery.accept");
    await click("processRecovery.confirmAccept");
    expect(text()).toContain("processRecovery.uncertain");
    expect(text()).not.toContain("processRecovery.accepted");
    expect(api.fetch).toHaveBeenCalledTimes(2);
    await click("processRecovery.retry");
    expect(text()).toContain("processRecovery.uncertain");
    expect(rows()).toHaveLength(1);
    // A returned record must carry every field the row shows.
    api.retry.mockResolvedValueOnce({
      ok: true,
      result: {
        owner: "daemon",
        result: "still_pending",
        record: { id: "a", version: "v2" },
        diagnostics: [],
      },
    });
    await click("processRecovery.retry");
    expect(text()).not.toContain("processRecovery.stillPending");
    expect(api.fetch).toHaveBeenCalledTimes(4);
  });

  it("leaves out a time the browser cannot represent and keeps the row", async () => {
    serve({ daemon: [listed("daemon", [record("a", { created_at_unix_seconds: 2 ** 64 - 1 })])] });
    await mount(["daemon"]);
    expect(rows()).toHaveLength(1);
    expect(text()).not.toContain("processRecovery.recordedAt");
    expect(text()).toContain("processRecovery.kinds.process_group");
  });

  it("stays open until a request it sent has settled", async () => {
    serve({ daemon: [listed("daemon", [record("a")]), listed("daemon", [])] });
    const reply = deferred<unknown>();
    api.accept.mockReturnValue(reply.promise);
    const onClose = await mount(["daemon"]);
    await click("processRecovery.accept");
    await click("processRecovery.confirmAccept");
    await act(async () =>
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
    );
    expect(onClose).not.toHaveBeenCalled();
    expect((document.body.querySelector("[data-modal-close]") as HTMLButtonElement).disabled).toBe(
      true,
    );
    await act(async () => reply.resolve({ ok: true, result: { owner: "daemon", accepted: true } }));
    await act(async () =>
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
    );
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("says only an administrator can handle the records", async () => {
    api.fetch.mockResolvedValue({
      ok: false,
      error: { code: "permission_denied", message: "denied" },
    });
    await mount(["daemon"]);
    expect(text()).toContain("processRecovery.denied");
    expect(rows()).toHaveLength(0);
  });

  it("does not read a malformed reply as an empty ledger", async () => {
    const { acceptance_supported: _, ...partial } = record("a");
    const malformed = [
      { owner: "daemon", records: [] },
      { owner: "daemon", records: [null], diagnostics: [] },
      { owner: "daemon", records: [partial], diagnostics: [] },
      { owner: "daemon", records: [], diagnostics: [{ reason: "other" }] },
    ];
    serve({ daemon: [...malformed.map((result) => ({ ok: true, result })), listed("daemon", [])] });
    await mount(["daemon"]);
    for (const result of malformed) {
      expect(text(), JSON.stringify(result)).toContain("processRecovery.unavailable");
      expect(text()).not.toContain("processRecovery.empty");
      await click("processRecovery.reload");
    }
    expect(text()).toContain("processRecovery.empty");
  });

  it("falls back to general wording for codes it does not know", async () => {
    serve({
      web: [
        listed(
          "web",
          [record("a", { kind: "future_kind", reason: "future_reason", origin: "future_origin" })],
          [{ reason: "future_reason", hint: "future_hint" }],
        ),
      ],
    });
    await mount(["web"]);
    for (const key of [
      "processRecovery.kinds.other",
      "processRecovery.reasons.other",
      "processRecovery.origins.unknown",
      "processRecovery.hints.other",
    ])
      expect(text()).toContain(key);
  });

  it("gives offline commands for the owner and fits retry_later to it", async () => {
    serve({
      web: [
        listed(
          "web",
          [record("a", { retryable: false, acceptance_supported: false })],
          [
            { reason: "evidence_unreadable", hint: "use_offline_review", relative_path: "x/y" },
            { reason: "cleanup_unconfirmed", hint: "retry_later" },
          ],
        ),
      ],
    });
    await mount(["web"]);
    const commands = document.body.querySelector("pre")?.textContent ?? "";
    expect(commands).toContain("cccc daemon process-recovery --owner web list");
    expect(commands).toContain("--accept-risk");
    expect(text()).toContain("processRecovery.offlineWebRunning");
    expect(text()).toContain("processRecovery.hints.retry_later_voice");
    expect(text()).toContain("x/y");
    // Neither action is offered for a record that supports neither.
    expect(rows()[0].querySelectorAll("button")).toHaveLength(0);
  });

  describe("accept all", () => {
    const three = () => [record("a"), record("b"), record("c")];

    it("is offered only for at least two acceptable records", async () => {
      serve({
        daemon: [listed("daemon", [record("a"), record("b", { acceptance_supported: false })])],
      });
      await mount(["daemon"]);
      expect(() => button('processRecovery.acceptAll:{"count":2}')).toThrow();
    });

    it("accepts one record at a time, rereading every shown owner in between", async () => {
      serve({
        daemon: [
          listed("daemon", three()),
          listed("daemon", [record("b"), record("c")]),
          listed("daemon", [record("c")]),
          listed("daemon", []),
        ],
        web: [listed("web", [])],
      });
      api.accept.mockResolvedValue({ ok: true, result: { owner: "daemon", accepted: true } });
      await mount(["daemon", "web"]);
      await click('processRecovery.acceptAll:{"count":3}');
      expect(api.accept).not.toHaveBeenCalled();
      await click("processRecovery.bulkConfirm");
      expect(api.accept.mock.calls.map((call) => call[1].id)).toEqual(["a", "b", "c"]);
      expect(api.fetch.mock.calls.filter((call) => call[0] === "web")).toHaveLength(4);
      expect(text()).toContain('processRecovery.bulkDone:{"count":3}');
    });

    it("finishes when something changes only after the last record", async () => {
      serve({
        daemon: [
          listed("daemon", [record("a"), record("b")]),
          listed("daemon", [record("b")]),
          listed("daemon", [], [{ reason: "other", hint: "retry_later" }]),
        ],
      });
      api.accept.mockResolvedValue({ ok: true, result: { owner: "daemon", accepted: true } });
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":2}');
      await click("processRecovery.bulkConfirm");
      expect(api.accept).toHaveBeenCalledTimes(2);
      expect(text()).toContain('processRecovery.bulkDone:{"count":2}');
      expect(text()).not.toContain("processRecovery.bulkStopped");
      expect(text()).toContain("processRecovery.hints.retry_later");
    });

    it("stops at a conflict and lists what it already accepted", async () => {
      serve({ daemon: [listed("daemon", three()), listed("daemon", [record("b"), record("c")])] });
      api.accept
        .mockResolvedValueOnce({ ok: true, result: { owner: "daemon", accepted: true } })
        .mockResolvedValueOnce({
          ok: false,
          error: { code: "process_recovery_changed", message: "changed" },
        });
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":3}');
      await click("processRecovery.bulkConfirm");
      expect(api.accept).toHaveBeenCalledTimes(2);
      expect(text()).toContain("processRecovery.bulkStopped.changed");
      expect(text()).toContain('processRecovery.bulkAcceptedBefore:{"count":1}');
    });

    it("stops when a new record appears or a pending one changes", async () => {
      serve({
        daemon: [
          listed("daemon", three()),
          listed("daemon", [record("b"), record("c"), record("d")]),
        ],
      });
      api.accept.mockResolvedValue({ ok: true, result: { owner: "daemon", accepted: true } });
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":3}');
      await click("processRecovery.bulkConfirm");
      expect(api.accept).toHaveBeenCalledOnce();
      expect(text()).toContain("processRecovery.bulkStopped.new_records");

      document.body.innerHTML = "";
      await act(async () => root!.unmount());
      root = null;
      api.accept.mockClear();
      serve({
        daemon: [
          listed("daemon", three()),
          listed("daemon", [record("b", { version: "v2" }), record("c")]),
        ],
      });
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":3}');
      await click("processRecovery.bulkConfirm");
      expect(api.accept).toHaveBeenCalledOnce();
      expect(text()).toContain("processRecovery.bulkStopped.changed");
    });

    it("stops when an accept returns no receipt", async () => {
      serve({ daemon: [listed("daemon", three())] });
      api.accept.mockResolvedValue({ ok: true, result: { owner: "daemon", accepted: false } });
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":3}');
      await click("processRecovery.bulkConfirm");
      expect(api.accept).toHaveBeenCalledOnce();
      expect(text()).toContain("processRecovery.bulkStopped.uncertain");
      expect(text()).not.toContain("processRecovery.bulkAcceptedBefore");
    });

    it("stops after the current record when asked to", async () => {
      serve({ daemon: [listed("daemon", three()), listed("daemon", [record("b"), record("c")])] });
      const first = deferred<unknown>();
      api.accept.mockReturnValueOnce(first.promise);
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":3}');
      await click("processRecovery.bulkConfirm");
      await click("processRecovery.bulkStop");
      await act(async () =>
        first.resolve({ ok: true, result: { owner: "daemon", accepted: true } }),
      );
      expect(api.accept).toHaveBeenCalledOnce();
      expect(text()).toContain("processRecovery.bulkStopped.user");
      expect(text()).toContain('processRecovery.bulkAcceptedBefore:{"count":1}');
    });

    it("stops when a reread fails", async () => {
      serve({
        daemon: [
          listed("daemon", three()),
          { ok: false, error: { code: "process_recovery_unavailable", message: "x" } },
        ],
      });
      api.accept.mockResolvedValue({ ok: true, result: { owner: "daemon", accepted: true } });
      await mount(["daemon"]);
      await click('processRecovery.acceptAll:{"count":3}');
      await click("processRecovery.bulkConfirm");
      expect(api.accept).toHaveBeenCalledOnce();
      expect(text()).toContain("processRecovery.bulkStopped.read_failed");
    });
  });
});
