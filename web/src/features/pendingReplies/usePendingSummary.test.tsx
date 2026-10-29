// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { usePendingSummary } from "./usePendingSummary";

const fetchSummary = vi.hoisted(() => vi.fn());
vi.mock("../../services/api/pendingReplies", () => ({ fetchPendingReplySummary: fetchSummary }));

type Value = ReturnType<typeof usePendingSummary>;
function Harness({
  enabled,
  groups,
  update,
}: {
  enabled: boolean;
  groups: string[];
  update: (v: Value) => void;
}) {
  const value = usePendingSummary(enabled, groups);
  update(value);
  return null;
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  fetchSummary.mockReset();
});
afterEach(() => vi.useRealTimers());

it("shows only allowed Groups and marks a failed refresh stale, then clears on lost authorization", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest: Value | undefined;
  const update = (value: Value) => (latest = value);
  fetchSummary.mockResolvedValueOnce({
    ok: true,
    result: {
      groups: [
        { group_id: "a", state: "fresh", pending: 2, revision: "r1" },
        { group_id: "b", state: "unknown", pending: null },
        { group_id: "secret", state: "fresh", pending: 7, revision: "r2" },
      ],
    },
  });
  try {
    await act(async () => root.render(<Harness enabled groups={["a", "b"]} update={update} />));
    expect(latest!.summaries.a).toMatchObject({ state: "fresh", pending: 2 });
    expect(latest!.summaries.b).toEqual({ state: "unknown", pending: null });
    expect(latest!.summaries.secret).toBeUndefined();

    fetchSummary.mockResolvedValueOnce({ ok: false, error: { code: "NETWORK_ERROR" } });
    await act(async () => latest!.refresh());
    expect(latest!.summaries.a).toMatchObject({ state: "stale", pending: 2 });
    expect(latest!.summaries.b).toEqual({ state: "unknown", pending: null });

    fetchSummary.mockResolvedValueOnce({ ok: false, error: { code: "permission_denied" } });
    await act(async () => latest!.refresh());
    expect(latest!.summaries.a).toEqual({ state: "unknown", pending: null, accessDenied: true });
    await act(async () => root.render(<Harness enabled={false} groups={["a"]} update={update} />));
    expect(latest!.summaries).toEqual({});
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("marks every Group unknown and unsupported, never zero, when the daemon lacks the summary", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest: Value | undefined;
  const update = (value: Value) => (latest = value);
  fetchSummary.mockResolvedValueOnce({
    ok: true,
    result: { groups: [{ group_id: "a", state: "fresh", pending: 2, revision: "r1" }] },
  });
  try {
    await act(async () => root.render(<Harness enabled groups={["a", "b"]} update={update} />));
    expect(latest!.summaries.a).toMatchObject({ state: "fresh", pending: 2 });

    fetchSummary.mockResolvedValueOnce({
      ok: false,
      error: { code: "unknown_op", message: "unknown operation: user_reply_requests_summary" },
    });
    await act(async () => latest!.refresh());
    const unsupported = { state: "unknown", pending: null, daemonUnsupported: true };
    expect(latest!.summaries.a).toEqual(unsupported);
    expect(latest!.summaries.b).toEqual(unsupported);

    fetchSummary.mockResolvedValueOnce({
      ok: true,
      result: { groups: [{ group_id: "a", state: "fresh", pending: 1, revision: "r2" }] },
    });
    await act(async () => latest!.refresh());
    expect(latest!.summaries.a).toMatchObject({ state: "fresh", pending: 1 });
    expect(latest!.summaries.a.daemonUnsupported).toBeUndefined();
    expect(latest!.summaries.b).toEqual({ state: "unknown", pending: null });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("discards an old asynchronous response after the permitted Group set changes", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest: Value | undefined;
  const update = (value: Value) => (latest = value);
  let finishOld!: (value: unknown) => void;
  fetchSummary.mockReturnValueOnce(new Promise((resolve) => (finishOld = resolve)));
  fetchSummary.mockResolvedValueOnce({
    ok: true,
    result: { groups: [{ group_id: "b", state: "fresh", pending: 1, revision: "new" }] },
  });
  try {
    await act(async () => root.render(<Harness enabled groups={["a"]} update={update} />));
    await act(async () => root.render(<Harness enabled groups={["b"]} update={update} />));
    expect(latest!.summaries.b).toMatchObject({ state: "fresh", pending: 1 });
    await act(async () =>
      finishOld({
        ok: true,
        result: { groups: [{ group_id: "a", state: "fresh", pending: 9, revision: "old" }] },
      }),
    );
    expect(latest!.summaries.a).toBeUndefined();
    expect(latest!.summaries.b).toMatchObject({ state: "fresh", pending: 1 });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
