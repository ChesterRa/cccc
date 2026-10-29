// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { PendingSummaryContext } from "./context";
import { pendingReplyView } from "./model";
import { usePendingPages } from "./usePendingPages";
import type { LedgerEvent } from "../../types";

const mocks = vi.hoisted(() => ({ page: vi.fn(), statuses: vi.fn() }));
vi.mock("../../services/api/pendingReplies", () => ({ fetchPendingReplies: mocks.page }));
vi.mock("../../services/api", () => ({ fetchLedgerStatuses: mocks.statuses }));

const request = (id: string, group = "g"): LedgerEvent => ({
  id,
  group_id: group,
  kind: "chat.message",
  by: "peer",
  data: { to: ["user"], message_mode: "request_reply", text: id },
});
const status = (closed = false) => ({
  obligation_status: {
    user: { reply_requested: true, replied: closed, cancelled: false, delivery_state: "" },
  },
  user_request: { question: "none", requester: "available" },
});
const page = (ids: string[], next: string | null, group = "g") => ({
  ok: true,
  result: {
    events: ids.map((id) => request(id, group)),
    statuses: Object.fromEntries(ids.map((id) => [id, status()])),
    has_more: !!next,
    before: next,
    total: ids.length,
  },
});
type Result = ReturnType<typeof usePendingPages>;

function Harness({ group, update }: { group: string; update: (value: Result) => void }) {
  const value = usePendingPages(group, true);
  update(value);
  return null;
}
function Wrapper({
  group,
  revision,
  update,
}: {
  group: string;
  revision?: string;
  update: (value: Result) => void;
}) {
  return (
    <PendingSummaryContext.Provider
      value={{
        summaries: { [group]: { state: "fresh", pending: 1, revision } },
        refresh: () => {},
      }}
    >
      <Harness group={group} update={update} />
    </PendingSummaryContext.Provider>
  );
}

it("uses the full paged projection, retains a newly closed row, and rebases an invalid cursor", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.page.mockReset();
  mocks.statuses.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Result;
  const update = (value: Result) => {
    latest = value;
  };
  mocks.page.mockResolvedValueOnce(page(["new"], "cursor-1"));
  try {
    await act(async () => root.render(<Wrapper group="g" update={update} />));
    expect(latest.events.map((event) => event.id)).toEqual(["new"]);
    mocks.page.mockResolvedValueOnce(page(["old"], null));
    await act(async () => latest.loadMore());
    expect(latest.events.map((event) => event.id)).toEqual(["old", "new"]);

    mocks.page.mockResolvedValueOnce(page(["newer"], "cursor-2"));
    mocks.statuses.mockResolvedValueOnce({
      ok: true,
      result: { statuses: { old: status(true), new: status() } },
    });
    await act(async () => root.render(<Wrapper group="g" revision="changed" update={update} />));
    expect(latest.events.map((event) => event.id)).toEqual(["old", "new", "newer"]);
    expect(pendingReplyView(latest.events[0])?.state).toBe("answered");
    expect(latest.hasMore).toBe(true);

    mocks.page.mockResolvedValueOnce({ ok: false, error: { code: "cursor_stale" } });
    mocks.page.mockResolvedValueOnce(page(["newer"], "cursor-3"));
    mocks.statuses.mockResolvedValueOnce({
      ok: true,
      result: { statuses: { old: status(true), new: status() } },
    });
    await act(async () => latest.loadMore());
    expect(latest.events.map((event) => event.id)).toEqual(["old", "new", "newer"]);
    expect(latest.before).toBe("cursor-3");
    expect(pendingReplyView(latest.events[0])?.state).toBe("answered");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("does not leak late data from a previous Group, or claim a failed status is current", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.page.mockReset();
  mocks.statuses.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Result;
  const update = (value: Result) => {
    latest = value;
  };
  let finish!: (response: unknown) => void;
  mocks.page.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  mocks.page.mockResolvedValueOnce(page(["b1"], null, "b"));
  try {
    await act(async () => root.render(<Wrapper group="a" update={update} />));
    await act(async () => root.render(<Wrapper group="b" update={update} />));
    await act(async () => finish(page(["a1"], null, "a")));
    expect(latest.events.map((event) => event.id)).toEqual(["b1"]);
    mocks.page.mockResolvedValueOnce({ ok: false, error: { code: "network_error" } });
    await act(async () => latest.refresh());
    expect(latest.error).toBe(true);
    expect(latest.daemonUnsupported).toBe(false);
    expect(pendingReplyView(latest.events[0])?.state).toBe("unknown");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("reports a daemon without the operation apart from other failures, until a read succeeds", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.page.mockReset();
  mocks.statuses.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Result;
  const update = (value: Result) => {
    latest = value;
  };
  mocks.page.mockResolvedValueOnce({
    ok: false,
    error: { code: "unknown_op", message: "unknown operation: user_reply_requests" },
  });
  try {
    await act(async () => root.render(<Wrapper group="g" update={update} />));
    expect(latest.error).toBe(true);
    expect(latest.daemonUnsupported).toBe(true);
    expect(latest.events).toEqual([]);

    mocks.page.mockResolvedValueOnce(page(["r1"], null));
    await act(async () => latest.refresh());
    expect(latest.error).toBe(false);
    expect(latest.daemonUnsupported).toBe(false);
    expect(latest.events.map((event) => event.id)).toEqual(["r1"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("fills a gap between retained older rows and a newer head in ledger order", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.page.mockReset();
  mocks.statuses.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Result;
  const update = (value: Result) => (latest = value);
  mocks.page.mockResolvedValueOnce(page(["a", "b"], null));
  try {
    await act(async () => root.render(<Wrapper group="g" update={update} />));
    mocks.page.mockResolvedValueOnce(page(["f", "g"], "cursor-1"));
    mocks.statuses.mockResolvedValueOnce({
      ok: true,
      result: { statuses: { a: status(), b: status() } },
    });
    await act(async () => root.render(<Wrapper group="g" revision="new" update={update} />));
    expect(latest.events.map((event) => event.id)).toEqual(["a", "b", "f", "g"]);

    mocks.page.mockResolvedValueOnce(page(["d", "e"], "cursor-2"));
    await act(async () => latest.loadMore());
    expect(latest.events.map((event) => event.id)).toEqual(["a", "b", "d", "e", "f", "g"]);

    mocks.page.mockResolvedValueOnce(page(["b", "c"], null));
    await act(async () => latest.loadMore());
    expect(latest.events.map((event) => event.id)).toEqual(["a", "b", "c", "d", "e", "f", "g"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
