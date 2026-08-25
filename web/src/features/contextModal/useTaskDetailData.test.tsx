// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../../services/api";
import type { Task } from "../../types";
import { useTaskDetailData } from "./useTaskDetailData";

vi.mock("../../services/api", () => ({ fetchTaskById: vi.fn() }));

type DetailResponse = Awaited<ReturnType<typeof api.fetchTaskById>>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

let detailApi: ReturnType<typeof useTaskDetailData> | null = null;

function Probe({ version, onLoaded }: { version: string; onLoaded: (task: Task) => void }) {
  detailApi = useTaskDetailData({
    groupId: "g-test",
    isOpen: true,
    onTaskLoaded: onLoaded,
    onTaskMissing: vi.fn(),
    getTasksVersion: () => version,
  });
  return <div>{detailApi.detailTasks.T001?.title || "missing"}</div>;
}

describe("useTaskDetailData", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fetchDetail = vi.mocked(api.fetchTaskById);

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    detailApi = null;
    fetchDetail.mockReset();
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("discards an old-version response and starts a fresh request for the new version", async () => {
    const stale = deferred<DetailResponse>();
    const fresh = deferred<DetailResponse>();
    const onLoaded = vi.fn();
    fetchDetail.mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);

    await act(async () => root.render(<Probe version="tasksv:1" onLoaded={onLoaded} />));
    const staleLoad = detailApi?.loadTaskDetail("T001");
    await act(async () => root.render(<Probe version="tasksv:2" onLoaded={onLoaded} />));
    const freshLoad = detailApi?.loadTaskDetail("T001");

    await act(async () =>
      stale.resolve({
        ok: true,
        result: {
          task: { id: "T001", title: "stale" },
          tasksVersion: "tasksv:1",
          deleteInfo: { allowed: true, total: 1, reason: "" },
        },
      }),
    );
    expect(await staleLoad).toBeNull();
    expect(host.textContent).toBe("missing");

    await act(async () =>
      fresh.resolve({
        ok: true,
        result: {
          task: { id: "T001", title: "fresh" },
          tasksVersion: "tasksv:2",
          deleteInfo: { allowed: true, total: 1, reason: "" },
        },
      }),
    );
    expect((await freshLoad)?.title).toBe("fresh");
    expect(onLoaded).toHaveBeenCalledOnce();
    expect(host.textContent).toBe("fresh");
    expect(fetchDetail).toHaveBeenCalledTimes(2);
  });
});
