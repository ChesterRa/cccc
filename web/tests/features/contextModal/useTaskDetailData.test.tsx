// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const apiMocks = vi.hoisted(() => ({ fetchTaskById: vi.fn() }));
vi.mock("../../../src/services/api", () => apiMocks);

import { useTaskDetailData } from "../../../src/features/contextModal/useTaskDetailData";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function Probe({
  groupId,
  onTaskLoaded,
  onTaskMissing,
}: {
  groupId: string;
  onTaskLoaded: ReturnType<typeof vi.fn>;
  onTaskMissing: ReturnType<typeof vi.fn>;
}) {
  const data = useTaskDetailData({ groupId, isOpen: true, onTaskLoaded, onTaskMissing });
  return (
    <div>
      <span data-testid="title">{data.detailTasks.T001?.title || ""}</span>
      <button type="button" data-testid="load" onClick={() => void data.loadTaskDetail("T001")} />
    </div>
  );
}

describe("useTaskDetailData", () => {
  it("ignores an old group's task_not_found after the new group has loaded the same id", async () => {
    const oldGroup = deferred<unknown>();
    const newGroup = deferred<unknown>();
    apiMocks.fetchTaskById.mockImplementation((groupId: string) =>
      groupId === "g-old" ? oldGroup.promise : newGroup.promise,
    );
    const onTaskLoaded = vi.fn();
    const onTaskMissing = vi.fn();
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () =>
      root.render(
        <Probe groupId="g-old" onTaskLoaded={onTaskLoaded} onTaskMissing={onTaskMissing} />,
      ),
    );
    await act(async () => host.querySelector<HTMLElement>('[data-testid="load"]')?.click());
    await vi.waitFor(() => expect(apiMocks.fetchTaskById).toHaveBeenCalledWith("g-old", "T001"));

    await act(async () =>
      root.render(
        <Probe groupId="g-new" onTaskLoaded={onTaskLoaded} onTaskMissing={onTaskMissing} />,
      ),
    );
    await act(async () => host.querySelector<HTMLElement>('[data-testid="load"]')?.click());
    await act(async () =>
      newGroup.resolve({
        ok: true,
        result: {
          task: { id: "T001", title: "New group task", status: "planned" },
          tasksVersion: "tasksv:2",
          deleteInfo: { allowed: true, total: 1, reason: "" },
        },
      }),
    );
    await vi.waitFor(() =>
      expect(host.querySelector('[data-testid="title"]')?.textContent).toBe("New group task"),
    );

    await act(async () =>
      oldGroup.resolve({ ok: false, error: { code: "task_not_found", message: "missing" } }),
    );

    expect(host.querySelector('[data-testid="title"]')?.textContent).toBe("New group task");
    expect(onTaskMissing).not.toHaveBeenCalled();
    await act(async () => root.unmount());
  });
});
