// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { PendingSummaryContext } from "../pendingReplies/context";
import type { Summary } from "../pendingReplies/usePendingSummary";
import { useGroupStore } from "../../stores";
import { useUIStore } from "../../stores/useUIStore";
import type { GroupMeta, LedgerEvent } from "../../types";
import { ChetReplyInbox, ChetWaitingBadge } from "./ChetReplyInbox";

const mocks = vi.hoisted(() => ({ page: vi.fn(), statuses: vi.fn() }));
vi.mock("../../services/api/pendingReplies", () => ({ fetchPendingReplies: mocks.page }));
vi.mock("../../services/api", () => ({ fetchLedgerStatuses: mocks.statuses }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) =>
      values ? `${key}:${JSON.stringify(values)}` : key,
    i18n: { language: "en" },
  }),
}));

const request = (id: string, group: string, by: string): LedgerEvent => ({
  id,
  group_id: group,
  kind: "chat.message",
  by,
  ts: "2026-11-26T00:00:00Z",
  data: { to: ["user"], message_mode: "request_reply", text: `ask ${id}` },
});
const pageOf = (group: string, events: LedgerEvent[]) => ({
  ok: true,
  result: {
    events,
    statuses: Object.fromEntries(
      events.map((event) => [
        event.id,
        {
          obligation_status: {
            user: { reply_requested: true, replied: false, cancelled: false, delivery_state: "" },
          },
          user_request: { question: "none", requester: "available" },
        },
      ]),
    ),
    has_more: false,
    before: null,
    total: events.length,
    group,
  },
});

async function mount(summaries: Record<string, Summary>, node: React.ReactNode) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <PendingSummaryContext.Provider value={{ summaries, refresh: () => {} }}>
        {node}
      </PendingSummaryContext.Provider>,
    ),
  );
  return {
    host,
    async unmount() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

it("gathers what every Group asked the user in one column, the longest-waiting Group first", async () => {
  useGroupStore.setState({
    groups: [
      { group_id: "g_new", title: "Newer" },
      { group_id: "g_old", title: "Older" },
      { group_id: "g_none", title: "Quiet" },
    ] as GroupMeta[],
  });
  mocks.statuses.mockResolvedValue({ ok: true, result: { statuses: {} } });
  mocks.page.mockImplementation(async (group: string) =>
    group === "g_old"
      ? pageOf(group, [request("e_old", group, "foreman")])
      : pageOf(group, [request("e_new", group, "peer-2")]),
  );
  const open = vi.fn();
  const view = await mount(
    {
      g_new: { state: "fresh", pending: 1, oldestTs: "2026-11-26T00:00:00Z" },
      g_old: { state: "fresh", pending: 1, oldestTs: "2026-11-24T00:00:00Z" },
      g_none: { state: "fresh", pending: 0 },
    },
    <ChetReplyInbox active onOpenGroupMessage={open} />,
  );
  try {
    await act(async () => {});
    const cards = [...view.host.querySelectorAll("[data-chet-waiting-request]")];
    expect(cards.map((card) => card.getAttribute("data-chet-waiting-request"))).toEqual([
      "e_old",
      "e_new",
    ]);
    expect(cards[0].textContent).toContain("Older");
    expect(cards[0].textContent).toContain("ask e_old");
    expect(cards[1].textContent).toContain("Newer");
    // A Group with nothing waiting is never asked for its requests.
    expect(mocks.page).not.toHaveBeenCalledWith("g_none", undefined, expect.anything());
    const openButton = [...cards[1].querySelectorAll("button")].find(
      (button) => button.textContent === "chet.waitingOpenInGroup",
    )!;
    await act(async () => openButton.click());
    expect(open).toHaveBeenCalledWith("g_new", "e_new");
  } finally {
    await view.unmount();
    mocks.page.mockReset();
  }
});

it("says plainly when nothing is waiting, without listing any Group", async () => {
  const view = await mount(
    { g: { state: "fresh", pending: 0 } },
    <ChetReplyInbox active onOpenGroupMessage={vi.fn()} />,
  );
  try {
    expect(view.host.querySelector("[data-chet-waiting-request]")).toBeNull();
    expect(view.host.textContent).toContain("chet.waitingEmpty");
    expect(mocks.page).not.toHaveBeenCalled();
  } finally {
    await view.unmount();
  }
});

it("counts the total once beside Chet and follows the waiting-prompts setting", async () => {
  const summaries: Record<string, Summary> = {
    a: { state: "fresh", pending: 2 },
    b: { state: "fresh", pending: 3 },
  };
  useUIStore.setState({ showPendingPrompts: true });
  const view = await mount(summaries, <ChetWaitingBadge />);
  try {
    const badge = () => view.host.querySelector("[data-chet-waiting-total]");
    expect(badge()?.getAttribute("data-chet-waiting-total")).toBe("5");
    expect(badge()?.textContent).toBe("5");
    await act(async () => useUIStore.getState().setShowPendingPrompts(false));
    expect(badge()).toBeNull();
  } finally {
    useUIStore.setState({ showPendingPrompts: true });
    localStorage.removeItem("cccc-pending-prompts");
    await view.unmount();
  }
  const none = await mount({ a: { state: "fresh", pending: 0 } }, <ChetWaitingBadge />);
  try {
    expect(none.host.querySelector("[data-chet-waiting-total]")).toBeNull();
  } finally {
    await none.unmount();
  }
});

it("reads no Group while the inbox is off screen, as when Chet is closed", async () => {
  mocks.page.mockReset();
  const view = await mount(
    { g: { state: "fresh", pending: 3, oldestTs: "2026-11-24T00:00:00Z" } },
    <ChetReplyInbox active={false} onOpenGroupMessage={vi.fn()} />,
  );
  try {
    await act(async () => {});
    expect(mocks.page).not.toHaveBeenCalled();
  } finally {
    await view.unmount();
  }
});

it("never turns a count it could not read into zero, and still reads that Group", async () => {
  mocks.page.mockReset();
  mocks.statuses.mockResolvedValue({ ok: true, result: { statuses: {} } });
  mocks.page.mockImplementation(async (group: string) =>
    pageOf(group, [request(`e_${group}`, group, "peer")]),
  );
  const summaries: Record<string, Summary> = {
    fresh: { state: "fresh", pending: 2, oldestTs: "2026-11-24T00:00:00Z" },
    lost: { state: "unknown", pending: null },
  };
  useUIStore.setState({ showPendingPrompts: true });
  const badge = await mount(summaries, <ChetWaitingBadge />);
  try {
    // An exact-looking "2" would hide the Group that could not be counted.
    expect(badge.host.textContent).toBe("2+?");
  } finally {
    await badge.unmount();
  }
  const onlyLost = await mount({ lost: { state: "unknown", pending: null } }, <ChetWaitingBadge />);
  try {
    expect(onlyLost.host.textContent).toBe("?");
  } finally {
    await onlyLost.unmount();
  }
  const inbox = await mount(summaries, <ChetReplyInbox active onOpenGroupMessage={vi.fn()} />);
  try {
    await act(async () => {});
    expect(inbox.host.textContent).toContain('chet.waitingUncertain:{"value":"2+?"}');
    expect(inbox.host.textContent).not.toContain("chet.waitingEmpty");
    const ids = [...inbox.host.querySelectorAll("[data-chet-waiting-request]")].map((card) =>
      card.getAttribute("data-chet-waiting-request"),
    );
    expect(ids).toEqual(["e_fresh", "e_lost"]);
  } finally {
    await inbox.unmount();
  }
});

it("offers a Group's earlier questions past the first page, and a retry when reading fails", async () => {
  mocks.page.mockReset();
  mocks.statuses.mockResolvedValue({ ok: true, result: { statuses: {} } });
  const first = pageOf("g", [request("e_2", "g", "peer")]);
  first.result.has_more = true;
  (first.result as { before: string | null }).before = "e_2";
  mocks.page
    .mockResolvedValueOnce(first)
    .mockResolvedValueOnce(pageOf("g", [request("e_1", "g", "peer")]));
  const view = await mount(
    { g: { state: "fresh", pending: 2, oldestTs: "2026-11-24T00:00:00Z" } },
    <ChetReplyInbox active onOpenGroupMessage={vi.fn()} />,
  );
  try {
    await act(async () => {});
    const more = view.host.querySelector<HTMLButtonElement>('[data-chet-waiting-more="g"]')!;
    expect(more).not.toBeNull();
    await act(async () => more.click());
    await act(async () => {});
    expect(mocks.page).toHaveBeenLastCalledWith("g", "e_2", expect.anything());
    const ids = [...view.host.querySelectorAll("[data-chet-waiting-request]")].map((card) =>
      card.getAttribute("data-chet-waiting-request"),
    );
    expect(ids.sort()).toEqual(["e_1", "e_2"]);
  } finally {
    await view.unmount();
  }

  mocks.page.mockReset();
  mocks.page
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce(pageOf("h", [request("e_h", "h", "peer")]));
  const failing = await mount(
    { h: { state: "fresh", pending: 1, oldestTs: "2026-11-24T00:00:00Z" } },
    <ChetReplyInbox active onOpenGroupMessage={vi.fn()} />,
  );
  try {
    await act(async () => {});
    const problem = failing.host.querySelector('[data-chet-waiting-problem="h"]')!;
    expect(problem.textContent).toContain("chet.waitingLoadFailed");
    const retry = [...problem.querySelectorAll("button")].find(
      (button) => button.textContent === "chet.waitingRetry",
    )!;
    await act(async () => retry.click());
    await act(async () => {});
    expect(failing.host.querySelector('[data-chet-waiting-request="e_h"]')).not.toBeNull();
    expect(failing.host.querySelector('[data-chet-waiting-problem="h"]')).toBeNull();
  } finally {
    await failing.unmount();
  }
});
