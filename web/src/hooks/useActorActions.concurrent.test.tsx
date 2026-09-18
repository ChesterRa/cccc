// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useActorActions } from "./useActorActions";
import { useUIStore, useGroupStore, useInboxStore, useModalStore } from "../stores";
import * as api from "../services/api";
import type { Actor, LedgerEvent } from "../types";

it("keeps overlapping Actor operations scoped while preserving unrelated global busy state", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useGroupStore.getState();
  useGroupStore.setState({
    refreshActors: vi.fn().mockResolvedValue(undefined),
    refreshGroups: vi.fn().mockResolvedValue(undefined),
    clearStreamingEventsForActor: vi.fn(),
  });
  useUIStore.setState({ actorBusy: {}, busy: "group-operation" });
  const replies: Array<() => void> = [];
  const start = vi
    .spyOn(api, "startActor")
    .mockImplementation(
      () =>
        new Promise((resolve) =>
          replies.push(() =>
            resolve({ ok: true, result: {} } as Awaited<ReturnType<typeof api.startActor>>),
          ),
        ),
    );
  let actions: ReturnType<typeof useActorActions>;
  function Probe({ groupId }: { groupId: string }) {
    actions = useActorActions(groupId);
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const a = { id: "a", running: false } as Actor;
  const b = { id: "b", running: false } as Actor;
  const pending: Promise<void>[] = [];
  try {
    await act(async () => root.render(<Probe groupId="g1" />));
    await act(async () => {
      pending.push(actions.toggleActorEnabled(a));
      pending.push(actions.toggleActorEnabled(b));
      void actions.toggleActorEnabled(a);
    });
    expect(start).toHaveBeenCalledTimes(2);
    expect(useUIStore.getState().actorBusy).toEqual({ '["g1","a"]': 1, '["g1","b"]': 1 });
    await act(async () => root.render(<Probe groupId="g2" />));
    await act(async () => {
      pending.push(actions.toggleActorEnabled(a));
    });
    expect(start).toHaveBeenCalledTimes(3);
    await act(async () => {
      replies[0]();
      await pending[0];
    });
    expect(useUIStore.getState().actorBusy).toEqual({ '["g1","b"]': 1, '["g2","a"]': 1 });
    expect(useUIStore.getState().busy).toBe("group-operation");
    await act(async () => {
      replies[1]();
      replies[2]();
      await Promise.all(pending);
    });
    expect(useUIStore.getState().actorBusy).toEqual({});
    expect(useUIStore.getState().busy).toBe("group-operation");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    useGroupStore.setState(original);
    useUIStore.setState({ actorBusy: {}, busy: "" });
  }
});

it("does not invalidate another Group's terminal when a pending restart finishes", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useGroupStore.getState();
  useGroupStore.setState({
    refreshActors: vi.fn().mockResolvedValue(undefined),
    refreshGroups: vi.fn().mockResolvedValue(undefined),
  });
  let finish: () => void;
  vi.spyOn(api, "restartActor").mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = () =>
          resolve({ ok: true, result: {} } as Awaited<ReturnType<typeof api.restartActor>>);
      }),
  );
  let actions: ReturnType<typeof useActorActions>;
  function Probe({ groupId }: { groupId: string }) {
    actions = useActorActions(groupId);
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let pending: Promise<void>;
  try {
    await act(async () => root.render(<Probe groupId="g1" />));
    await act(async () => {
      pending = actions.relaunchActor({ id: "a", running: true } as Actor);
    });
    await act(async () => root.render(<Probe groupId="g2" />));
    await act(async () => {
      finish();
      await pending;
    });
    expect(actions!.getTermEpoch("a")).toBe(0);
    await act(async () => root.render(<Probe groupId="g1" />));
    expect(actions!.getTermEpoch("a")).toBe(1);
    expect(useUIStore.getState().actorBusy).toEqual({});
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    useGroupStore.setState(original);
  }
});

for (const nextView of ["same-actor-other-group", "other-actor-same-group", "unchanged"]) {
  it(`finishes Actor removal without replacing a newer selection: ${nextView}`, async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const original = useGroupStore.getState();
    const originalUI = useUIStore.getState();
    const loadGroup = vi.fn().mockResolvedValue(undefined);
    const refreshActors = vi.fn().mockResolvedValue(undefined);
    useGroupStore.setState({
      selectedGroupId: "g1",
      loadGroup,
      refreshActors,
      refreshGroups: vi.fn().mockResolvedValue(undefined),
      clearStreamingEventsForActor: vi.fn(),
    });
    useUIStore.setState({ activeTab: "a", actorBusy: {} });
    vi.stubGlobal(
      "confirm",
      vi.fn(() => true),
    );
    let finish!: () => void;
    vi.spyOn(api, "removeActor").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = () =>
            resolve({ ok: true, result: {} } as Awaited<ReturnType<typeof api.removeActor>>);
        }),
    );
    let actions!: ReturnType<typeof useActorActions>;
    function Probe() {
      actions = useActorActions("g1");
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    let pending!: Promise<void>;
    try {
      await act(async () => root.render(<Probe />));
      await act(async () => {
        pending = actions.removeActor({ id: "a" } as Actor, "a");
      });
      if (nextView === "same-actor-other-group") useGroupStore.setState({ selectedGroupId: "g2" });
      if (nextView === "other-actor-same-group") useUIStore.setState({ activeTab: "b" });
      await act(async () => {
        finish();
        await pending;
      });
      expect(useUIStore.getState().activeTab).toBe(
        nextView === "unchanged" ? "chat" : nextView === "other-actor-same-group" ? "b" : "a",
      );
      if (nextView === "same-actor-other-group") expect(loadGroup).not.toHaveBeenCalled();
      else expect(loadGroup).toHaveBeenCalledWith("g1");
      expect(refreshActors).toHaveBeenCalledWith("g1");
      expect(useUIStore.getState().actorBusy).toEqual({});
    } finally {
      await act(async () => root.unmount());
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
      useGroupStore.setState(original);
      useUIStore.setState(originalUI);
    }
  });
}

it("does not let a late inbox response replace a newly opened inbox", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const originalInbox = useInboxStore.getState();
  const originalModal = useModalStore.getState();
  const replies: Array<(text: string) => void> = [];
  vi.spyOn(api, "fetchInbox").mockImplementation(
    () =>
      new Promise((resolve) => {
        replies.push((text) =>
          resolve({ ok: true, result: { messages: [{ id: text } as LedgerEvent] } } as Awaited<
            ReturnType<typeof api.fetchInbox>
          >),
        );
      }),
  );
  let actions!: ReturnType<typeof useActorActions>;
  function Probe() {
    actions = useActorActions("g1");
    return null;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  const pending: Promise<void>[] = [];
  try {
    await act(async () => root.render(<Probe />));
    await act(async () => {
      pending.push(actions.openActorInbox({ id: "a" } as Actor));
    });
    useModalStore.getState().closeModal("inbox");
    await act(async () => {
      pending.push(actions.openActorInbox({ id: "b" } as Actor));
    });
    await act(async () => {
      replies[1]("new");
      await pending[1];
    });
    await act(async () => {
      replies[0]("old");
      await pending[0];
    });
    expect(useInboxStore.getState().inboxMessages.map((m) => m.id)).toEqual(["new"]);
  } finally {
    await act(async () => root.unmount());
    vi.restoreAllMocks();
    useInboxStore.setState(originalInbox);
    useModalStore.setState(originalModal);
  }
});
