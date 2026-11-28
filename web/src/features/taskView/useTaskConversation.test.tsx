// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";

// Network boundary: the Group's message search.
const api = vi.hoisted(() => ({ searchChatMessages: vi.fn() }));
vi.mock("../../services/api", () => api);

import { MAX_TASK_SEARCH_PAGES, useTaskConversation } from "./useTaskConversation";

type View = ReturnType<typeof useTaskConversation>;
let view: View;
function Probe({ groupId, taskId }: { groupId: string; taskId: string }) {
  view = useTaskConversation(groupId, taskId);
  return null;
}

const message = (id: string): LedgerEvent => ({
  id,
  kind: "chat.message",
  group_id: "g1",
  by: "worker",
  data: { text: `T98 ${id}` },
});
const page = (events: LedgerEvent[], hasMore: boolean) => ({
  ok: true,
  result: { events, has_more: hasMore, count: events.length },
});

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
});

async function mount(taskId: string) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Probe groupId="g1" taskId={taskId} />));
  return {
    rerender: (next: string) => act(async () => root.render(<Probe groupId="g1" taskId={next} />)),
  };
}

it("reads nothing while no task is open", async () => {
  await mount("");
  expect(view.active).toBe(false);
  expect(api.searchChatMessages).not.toHaveBeenCalled();
});

it("reads every page of the history, oldest first", async () => {
  api.searchChatMessages
    .mockResolvedValueOnce(page([message("c"), message("d")], true))
    .mockResolvedValueOnce(page([message("a"), message("b")], false));
  await mount("T98");
  expect(api.searchChatMessages).toHaveBeenNthCalledWith(1, "g1", "T98", {
    limit: 200,
    before: undefined,
  });
  // The next page ends before the oldest message already read.
  expect(api.searchChatMessages).toHaveBeenNthCalledWith(2, "g1", "T98", {
    limit: 200,
    before: "c",
  });
  expect(view.events.map((event) => event.id)).toEqual(["a", "b", "c", "d"]);
  expect(view).toMatchObject({ loaded: true, loading: false, error: false, truncated: false });
});

it("stops at the page limit and says the view is partial", async () => {
  let n = 0;
  api.searchChatMessages.mockImplementation(async () => page([message(`m${(n += 1)}`)], true));
  await mount("T98");
  expect(api.searchChatMessages).toHaveBeenCalledTimes(MAX_TASK_SEARCH_PAGES);
  expect(view.truncated).toBe(true);
  expect(view.loaded).toBe(true);
});

it("reports a failed read and reads again on retry", async () => {
  api.searchChatMessages.mockResolvedValueOnce({ ok: false, error: { message: "down" } });
  await mount("T98");
  expect(view).toMatchObject({ error: true, loading: false, loaded: false });
  api.searchChatMessages.mockResolvedValueOnce(page([message("a")], false));
  await act(async () => view.refresh());
  expect(view).toMatchObject({ error: false, loaded: true });
  expect(view.events.map((event) => event.id)).toEqual(["a"]);
});

it("drops a slow answer for a task the view has left", async () => {
  let release!: (value: unknown) => void;
  api.searchChatMessages.mockImplementationOnce(
    () => new Promise((resolve) => (release = resolve)),
  );
  const mounted = await mount("T98");
  api.searchChatMessages.mockResolvedValueOnce(page([message("other")], false));
  await mounted.rerender("T99");
  await act(async () => release(page([message("stale")], false)));
  expect(view.events.map((event) => event.id)).toEqual(["other"]);
});
