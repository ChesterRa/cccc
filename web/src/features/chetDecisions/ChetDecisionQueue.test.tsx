// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";

// Network boundary: each Group's list of open questions.
const api = vi.hoisted(() => ({ fetchPendingReplies: vi.fn() }));
vi.mock("../../services/api/pendingReplies", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api/pendingReplies")>()),
  fetchPendingReplies: api.fetchPendingReplies,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}:${JSON.stringify(options)}` : key,
  }),
}));

import { PendingSummaryContext } from "../pendingReplies/context";
import { useGroupStore } from "../../stores";
import { setUiStylePrefs } from "../../utils/uiStyle";
import { ChetDecisionQueue } from "./ChetDecisionQueue";
import { ChetDecisionsEntry } from "./ChetDecisionsEntry";
import { useChetDecisionsStore } from "./chetDecisionsStore";

const DAY = 24 * 60 * 60 * 1000;
function request(id: string, groupId: string, daysAgo: number, text: string): LedgerEvent {
  return {
    id,
    kind: "chat.message",
    group_id: groupId,
    by: "worker",
    ts: new Date(Date.now() - daysAgo * DAY).toISOString(),
    data: { text, to: ["user"], message_mode: "request_reply" },
  };
}
const page = (events: LedgerEvent[], hasMore = false) => ({
  ok: true,
  result: {
    events,
    statuses: Object.fromEntries(events.map((event) => [event.id, {}])),
    total: events.length,
    has_more: hasMore,
    before: hasMore ? events[0]!.id : null,
  },
});

let host: HTMLDivElement;
let root: Root;
const refresh = vi.fn();
const summaries = {
  g1: { state: "fresh" as const, pending: 2 },
  g2: { state: "fresh" as const, pending: 0 },
  g3: { state: "unknown" as const, pending: null },
};

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.clearAllMocks();
  useGroupStore.setState({
    groups: [
      { group_id: "g1", title: "Design" },
      { group_id: "g2", title: "Quiet" },
      { group_id: "g3", title: "Billing" },
    ],
  });
  api.fetchPendingReplies.mockImplementation(async (groupId: string) =>
    groupId === "g1"
      ? page([request("new", "g1", 1, "Ship it?"), request("old", "g1", 12, "Rename the repo?")])
      : { ok: false, error: { message: "down" } },
  );
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  useChetDecisionsStore.getState().setOpen(false);
  setUiStylePrefs({ style: "lcd" });
});

type SummaryValue = NonNullable<React.ContextType<typeof PendingSummaryContext>>;
const withSummary = (node: React.ReactNode, value: SummaryValue = { summaries, refresh }) => (
  <PendingSummaryContext.Provider value={value}>{node}</PendingSummaryContext.Provider>
);
const cards = () => [...host.querySelectorAll<HTMLElement>("[data-chet-decision]")];
const filterButton = (name: string) =>
  host.querySelector<HTMLButtonElement>(`[data-chet-decision-filter="${name}"]`)!;

async function renderQueue(onOpen = vi.fn(), onClose = vi.fn()) {
  await act(async () =>
    root.render(withSummary(<ChetDecisionQueue onOpenGroupMessage={onOpen} onClose={onClose} />)),
  );
  return { onOpen, onClose };
}

it("gathers every Group's open questions, skipping Groups known to have none", async () => {
  await renderQueue();
  // g2 has a known zero and is not read; g3's count is unknown, so it is.
  expect(api.fetchPendingReplies.mock.calls.map((call) => call[0]).sort()).toEqual(["g1", "g3"]);
  expect(cards().map((card) => card.dataset.chetDecision)).toEqual(["new", "old"]);
  expect(cards()[0]!.textContent).toContain("Design");
  expect(cards()[0]!.textContent).toContain("Ship it?");
  // Only the question past the stale line is marked.
  expect(cards()[0]!.querySelector("[data-chet-decision-stale-tag]")).toBeNull();
  expect(cards()[1]!.querySelector("[data-chet-decision-stale-tag]")).not.toBeNull();
  // The Group that could not be read is reported, not silently missing.
  expect(host.querySelector('[role="status"]')?.textContent).toContain(
    'chetDecisions.someFailed:{"count":1}',
  );
});

it("filters to what needs deciding or what may have expired, with counts", async () => {
  await renderQueue();
  expect(filterButton("all").textContent).toContain("2");
  expect(filterButton("stale").textContent).toContain("1");
  await act(async () => filterButton("stale").click());
  expect(cards().map((card) => card.dataset.chetDecision)).toEqual(["old"]);
  await act(async () => filterButton("fresh").click());
  expect(cards().map((card) => card.dataset.chetDecision)).toEqual(["new"]);
});

it("moves between questions with J and K, but not while typing", async () => {
  await renderQueue();
  const press = (key: string, target: Element | Document = document) =>
    act(() => {
      target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
    });
  press("j");
  expect(cards()[0]!.contains(document.activeElement)).toBe(true);
  press("j");
  expect(cards()[1]!.contains(document.activeElement)).toBe(true);
  press("k");
  expect(cards()[0]!.contains(document.activeElement)).toBe(true);
  const field = document.createElement("textarea");
  cards()[0]!.append(field);
  field.focus();
  press("j", field);
  expect(document.activeElement).toBe(field);
});

it("opens a question in its Group, reads again on refresh and goes back on request", async () => {
  const { onOpen, onClose } = await renderQueue();
  const button = (text: string) =>
    [...host.querySelectorAll("button")].find((item) => item.textContent === text)!;
  await act(async () => cards()[1]!.querySelector<HTMLButtonElement>("header button")!.click());
  expect(onOpen).toHaveBeenCalledWith("g1", "old");
  api.fetchPendingReplies.mockClear();
  await act(async () => button("chetDecisions.refresh").click());
  expect(refresh).toHaveBeenCalledTimes(1);
  expect(api.fetchPendingReplies).toHaveBeenCalled();
  await act(async () => button("chetDecisions.back").click());
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("offers the way in only in the preview style, and only while something waits", async () => {
  const entry = () => host.querySelector<HTMLButtonElement>("[data-chet-decisions-entry]");
  await act(async () => root.render(withSummary(<ChetDecisionsEntry />)));
  expect(entry()).toBeNull();
  await act(async () => setUiStylePrefs({ style: "preview" }));
  // Two known questions, plus a Group whose count is unknown.
  expect(entry()?.textContent).toContain('chetDecisions.entryCount:{"count":2}+');
  await act(async () => entry()!.click());
  expect(useChetDecisionsStore.getState().open).toBe(true);
  await act(async () =>
    root.render(
      withSummary(<ChetDecisionsEntry />, {
        // Nothing known to wait; one Group's count cannot be read.
        summaries: { g1: { state: "fresh", pending: 0 }, g3: { state: "unknown", pending: null } },
        refresh,
      }),
    ),
  );
  expect(entry()).toBeNull();
});

it("holds every card, labels and all, inside its own scroll area", async () => {
  await renderQueue();
  const list = host.querySelector<HTMLElement>("[data-chet-decision-list]")!;
  // Absolutely placed descendants (the screen-reader-only labels of each answer card) resolve
  // against the nearest positioned ancestor; the list must be that ancestor, or they extend
  // Chet's outer body and let it scroll the composer out of view.
  expect(getComputedStyle(list).position).toBe("relative");
  expect(getComputedStyle(list).overflowY).not.toBe("visible");
  for (const card of cards()) expect(list.contains(card)).toBe(true);
});
