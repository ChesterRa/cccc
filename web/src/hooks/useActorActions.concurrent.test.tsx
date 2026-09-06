// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useActorActions } from "./useActorActions";
import { useUIStore, useGroupStore } from "../stores";
import * as api from "../services/api";
import type { Actor } from "../types";

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
