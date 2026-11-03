// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";

const api = vi.hoisted(() => ({
  fetchPinnedIds: vi.fn(),
  fetchPinnedMessages: vi.fn(),
  setMessagePinned: vi.fn(),
}));
vi.mock("../../services/api/pins", () => api);

import { loadPinnedIds, setPinned, usePinStore } from "./pinStore";
import { usePinnedMessages } from "./usePinnedMessages";

type View = ReturnType<typeof usePinnedMessages>;
let view: View;

function Probe({ groupId, active }: { groupId: string; active: boolean }) {
  view = usePinnedMessages(groupId, active);
  return null;
}

const message = (id: string): LedgerEvent => ({
  id,
  kind: "chat.message",
  group_id: "g1",
  by: "worker",
  data: { text: id },
});
const at = "2026-11-03T00:00:00Z";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  usePinStore.setState({ groups: {} });
  vi.clearAllMocks();
});

async function mount(active: boolean) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Probe groupId="g1" active={active} />));
  return {
    rerender: (next: boolean) => act(async () => root.render(<Probe groupId="g1" active={next} />)),
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

it("lists a message pinned just before the view opens", async () => {
  const change = deferred<unknown>();
  api.setMessagePinned.mockReturnValue(change.promise);
  api.fetchPinnedMessages.mockResolvedValue({
    ok: true,
    result: { pins: [{ event_id: "m1", pinned_at: at, event: message("m1") }] },
  });
  const pinning = setPinned("g1", "m1", true);
  const probe = await mount(true);
  try {
    // The read waits for the pin to be answered rather than racing it.
    expect(api.fetchPinnedMessages).not.toHaveBeenCalled();
    expect(view.loading).toBe(true);
    await act(async () => {
      change.resolve({ ok: true, result: { pins: [{ event_id: "m1", pinned_at: at }] } });
      await pinning;
    });
    expect(api.fetchPinnedMessages).toHaveBeenCalledOnce();
    expect(view.events.map((event) => event.id)).toEqual(["m1"]);
    expect(usePinStore.getState().groups.g1.ids).toEqual(["m1"]);
  } finally {
    await probe.unmount();
  }
});

it("shows a pinned slash command as the conversation does, not its internal dispatch text", async () => {
  const dispatch: LedgerEvent = {
    id: "m2",
    kind: "chat.message",
    group_id: "g1",
    by: "user",
    data: {
      text: "[CCCC] INTERNAL CONTROL: capability skill dispatch",
      refs: [
        {
          kind: "control",
          hidden: true,
          control_kind: "slash_skill_dispatch",
          command: "/review",
          task_text: "the pin batch",
        },
      ],
    },
  };
  api.fetchPinnedMessages.mockResolvedValue({
    ok: true,
    result: { pins: [{ event_id: "m2", pinned_at: at, event: dispatch }] },
  });
  const probe = await mount(true);
  try {
    expect(view.events).toHaveLength(1);
    expect((view.events[0].data as { text?: string }).text).toBe("/review the pin batch");
  } finally {
    await probe.unmount();
  }
});

it("keeps an unpinned row until the view is left, and drops the notice for a removed pin", async () => {
  api.fetchPinnedMessages.mockResolvedValue({
    ok: true,
    result: {
      pins: [
        { event_id: "m1", pinned_at: at, event: message("m1") },
        { event_id: "gone", pinned_at: at, event: null },
      ],
    },
  });
  const probe = await mount(true);
  try {
    expect(view.events.map((event) => event.id)).toEqual(["m1"]);
    expect(view.unavailable).toEqual(["gone"]);
    api.setMessagePinned
      .mockResolvedValueOnce({ ok: true, result: { pins: [{ event_id: "gone", pinned_at: at }] } })
      .mockResolvedValueOnce({ ok: true, result: { pins: [] } });
    await act(async () => {
      await setPinned("g1", "m1", false);
    });
    expect(view.events.map((event) => event.id)).toEqual(["m1"]);
    expect(view.unavailable).toEqual(["gone"]);
    await act(async () => {
      await setPinned("g1", "gone", false);
    });
    expect(view.unavailable).toEqual([]);
    expect(api.fetchPinnedMessages).toHaveBeenCalledOnce();

    await probe.rerender(false);
    expect(view).toMatchObject({ active: false, events: [], unavailable: [] });
  } finally {
    await probe.unmount();
  }
});

it("keeps what it showed when reading again fails, under the error", async () => {
  api.fetchPinnedMessages.mockResolvedValueOnce({
    ok: true,
    result: { pins: [{ event_id: "m1", pinned_at: at, event: message("m1") }] },
  });
  const probe = await mount(true);
  try {
    api.fetchPinnedMessages.mockResolvedValueOnce({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "offline" },
    });
    await act(async () => {
      view.refresh();
    });
    expect(view.error).toBe(true);
    expect(view.events.map((event) => event.id)).toEqual(["m1"]);
  } finally {
    await probe.unmount();
  }
});

it("keeps the newer list when an older read of the Group's pins answers last", async () => {
  // The Group's IDs are read before another device pins m1; the Pinned view reads after it.
  const older = deferred<unknown>();
  api.fetchPinnedIds.mockReturnValue(older.promise);
  const loading = loadPinnedIds("g1");
  api.fetchPinnedMessages.mockResolvedValue({
    ok: true,
    result: { pins: [{ event_id: "m1", pinned_at: at, event: message("m1") }] },
  });
  const probe = await mount(true);
  try {
    expect(view.events.map((event) => event.id)).toEqual(["m1"]);
    await act(async () => {
      older.resolve({ ok: true, result: { pins: [] } });
      await loading;
    });
    expect(view.events.map((event) => event.id)).toEqual(["m1"]);
    expect(usePinStore.getState().groups.g1.ids).toEqual(["m1"]);
  } finally {
    await probe.unmount();
  }
});
