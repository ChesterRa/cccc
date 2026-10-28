// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { PendingSummaryContext } from "./context";
import { useLivePendingStatuses } from "./useLivePendingStatuses";

const fetchStatuses = vi.hoisted(() => vi.fn());
vi.mock("../../services/api", () => ({ fetchLedgerStatuses: fetchStatuses }));

const event = (group: string): LedgerEvent => ({
  id: "request",
  group_id: group,
  kind: "chat.message",
  by: "peer",
  data: { to: ["user"], message_mode: "request_reply", text: "Question" },
});
type Statuses = ReturnType<typeof useLivePendingStatuses>;
function Harness({
  group,
  revision,
  state,
  update,
  events,
}: {
  group: string;
  revision: string;
  state: "fresh" | "stale";
  update: (result: Statuses) => void;
  events?: LedgerEvent[];
}) {
  return (
    <PendingSummaryContext.Provider
      value={{ summaries: { [group]: { state, pending: 1, revision } }, refresh: () => {} }}
    >
      <Consumer group={group} update={update} events={events} />
    </PendingSummaryContext.Provider>
  );
}
function Consumer({
  group,
  update,
  events,
}: {
  group: string;
  update: (result: Statuses) => void;
  events?: LedgerEvent[];
}) {
  update(useLivePendingStatuses(group, events ?? [event(group)], true));
  return null;
}

it("keeps verified rows usable during a revision refresh but waits for new rows", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  fetchStatuses.mockReset();
  fetchStatuses.mockResolvedValueOnce({
    ok: true,
    result: {
      statuses: { request: { user_request: { question: "none", requester: "available" } } },
    },
  });
  let finish!: (response: unknown) => void;
  fetchStatuses.mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Statuses;
  const update = (result: Statuses) => (latest = result);
  try {
    await act(async () =>
      root.render(<Harness group="g" revision="v1" state="fresh" update={update} />),
    );
    expect(latest.request?.checking).toBe(false);
    await act(async () =>
      root.render(
        <Harness
          group="g"
          revision="v2"
          state="fresh"
          events={[event("g"), { ...event("g"), id: "new" }]}
          update={update}
        />,
      ),
    );
    expect(latest.request?.checking).toBe(false);
    expect(latest.new?.checking).toBe(true);
    await act(async () =>
      finish({
        ok: true,
        result: {
          statuses: {
            request: { user_request: { question: "none", requester: "unavailable" } },
            new: { user_request: { question: "none", requester: "available" } },
          },
        },
      }),
    );
    expect(latest.request?.status?.user_request?.requester).toBe("unavailable");
    expect(latest.request?.checking).toBe(false);
    expect(latest.new?.checking).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("loads new messages in one batch and invalidates old-group or stale status", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  fetchStatuses.mockReset();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Statuses;
  const update = (result: Statuses) => {
    latest = result;
  };
  let finishOld!: (response: unknown) => void;
  fetchStatuses.mockReturnValueOnce(
    new Promise((resolve) => {
      finishOld = resolve;
    }),
  );
  fetchStatuses.mockResolvedValueOnce({
    ok: true,
    result: {
      statuses: { request: { user_request: { question: "none", requester: "available" } } },
    },
  });
  try {
    await act(async () =>
      root.render(<Harness group="a" revision="v1" state="fresh" update={update} />),
    );
    expect(latest.request).toEqual({ status: null, checking: true });
    await act(async () =>
      root.render(<Harness group="b" revision="v2" state="fresh" update={update} />),
    );
    expect(latest.request?.status?.user_request?.requester).toBe("available");
    expect(latest.request?.checking).toBe(false);
    await act(async () =>
      finishOld({
        ok: true,
        result: {
          statuses: { request: { user_request: { question: "none", requester: "unsupported" } } },
        },
      }),
    );
    expect(latest.request?.status?.user_request?.requester).toBe("available");
    await act(async () =>
      root.render(<Harness group="b" revision="v2" state="stale" update={update} />),
    );
    expect(latest.request?.status?.user_request?.requester).toBe("available");
    expect(latest.request?.checking).toBe(true);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("retries a failed status read without requiring another ledger event", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  fetchStatuses.mockReset();
  fetchStatuses.mockResolvedValueOnce({ ok: false, error: { code: "NETWORK_ERROR" } });
  fetchStatuses.mockResolvedValueOnce({
    ok: true,
    result: {
      statuses: { request: { user_request: { question: "none", requester: "available" } } },
    },
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let latest!: Statuses;
  try {
    await act(async () =>
      root.render(
        <Harness
          group="g"
          revision="v1"
          state="fresh"
          update={(value) => {
            latest = value;
          }}
        />,
      ),
    );
    expect(latest.request).toEqual({ status: null, checking: true });
    await act(async () => vi.advanceTimersByTimeAsync(5000));
    expect(latest.request?.status?.user_request?.requester).toBe("available");
    expect(latest.request?.checking).toBe(false);
    expect(fetchStatuses).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  }
});
