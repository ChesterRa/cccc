// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";

// Network boundary: the Group's message search, which a task's view reads.
const search = vi.hoisted(() => vi.fn());
vi.mock("../../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api")>()),
  searchChatMessages: search,
}));

import { useChatMessageView } from "./useChatMessageView";

type View = ReturnType<typeof useChatMessageView>;
let view: View;

function message(id: string, text: string, extra: Record<string, unknown> = {}): LedgerEvent {
  return {
    id,
    kind: "chat.message",
    group_id: "g1",
    by: "worker",
    ts: `2026-11-27T00:00:0${id.length}Z`,
    data: { text, to: ["user"], ...extra },
  };
}
const loaded = [
  message("aa", "about T98", {}),
  message("bbb", "lunch?"),
  message("cccc", "on it", { reply_to: "aa" }),
];

function Probe({ taskId, events }: { taskId: string; events: LedgerEvent[] }) {
  view = useChatMessageView({
    selectedGroupId: "g1",
    liveJump: null,
    events,
    streamingEvents: [],
    outboxEntries: [],
    chatWindow: null,
    chatFilter: "all",
    taskId,
    scrollSnapshot: null,
    hasLoadedTail: true,
    hasMoreHistory: true,
    isLoadingHistory: false,
    isChatWindowLoading: false,
    groupDoc: null,
    groupContext: null,
    groupSettings: null,
    actors: [],
    needsActors: false,
  });
  return null;
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  search.mockReset();
});

async function mount(taskId: string) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (task: string, events = loaded) =>
    act(async () => root.render(<Probe taskId={task} events={events} />));
  await render(taskId);
  return { render };
}
const shown = () => view.chatMessages.map((event) => event.id);

it("shows the whole conversation and reads no search while no task is open", async () => {
  await mount("");
  expect(shown()).toEqual(["aa", "bbb", "cccc"]);
  expect(view.taskViewActive).toBe(false);
  expect(search).not.toHaveBeenCalled();
});

it("shows only the task's messages: older ones from the search, loaded ones and their replies", async () => {
  search.mockResolvedValue({
    ok: true,
    result: { events: [message("z", "T98 kickoff, long ago")], has_more: false, count: 1 },
  });
  const mounted = await mount("T98");
  expect(search).toHaveBeenCalledWith(
    "g1",
    "",
    { taskId: "T98", limit: 200, before: undefined },
    { signal: expect.any(AbortSignal) },
  );
  expect(shown()).toEqual(["z", "aa", "cccc"]);
  expect(view.taskViewActive).toBe(true);
  // The view is complete in one read: there is no older page to ask the list for.
  expect(view.effectiveHasMoreHistory).toBe(false);
  expect(view.chatViewKey).toBe("g1:task:T98");

  // A message that arrives while the view is open joins it without another search.
  search.mockClear();
  await mounted.render("T98", [...loaded, message("ddddd", "T98 shipped")]);
  expect(shown()).toEqual(["z", "aa", "cccc", "ddddd"]);
  expect(search).not.toHaveBeenCalled();

  // Leaving the view returns the full conversation under its own key.
  await mounted.render("");
  expect(shown()).toEqual(["aa", "bbb", "cccc"]);
  expect(view.chatViewKey).not.toContain(":task:");
});

it("reports a failed read instead of an empty task", async () => {
  search.mockResolvedValue({ ok: false, error: { message: "down" } });
  await mount("T404");
  expect(view.taskViewError).toBe(true);
  expect(shown()).toEqual([]);
  expect(view.chatEmptyState).toBe("filtered_empty");
});
