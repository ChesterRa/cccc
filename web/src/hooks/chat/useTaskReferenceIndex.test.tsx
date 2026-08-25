// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../../services/api";
import type { LedgerEvent, Task } from "../../types";
import { useTaskReferenceIndex } from "./useTaskReferenceIndex";

vi.mock("../../services/api", () => ({ fetchTasksByIds: vi.fn() }));

const task = (id: string, title: string): Task => ({ id, title });
const event = (id: string): LedgerEvent => ({
  data: { refs: [{ kind: "task_ref", task_id: id }] },
});

function Probe({ events, seedTasks }: { events: LedgerEvent[]; seedTasks: Task[] }) {
  const index = useTaskReferenceIndex({
    groupId: "g-test",
    events,
    tasksVersion: "tasksv:2",
    seedTasks,
  });
  return <div>{`${index.get("T001")?.title}|${index.get("T101")?.title}`}</div>;
}

describe("useTaskReferenceIndex", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fetchByIds = vi.mocked(api.fetchTasksByIds);

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    fetchByIds.mockReset();
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  });

  it("keeps failed-batch cache entries and retries the incomplete refresh", async () => {
    const ids = Array.from({ length: 101 }, (_, index) => `T${String(index + 1).padStart(3, "0")}`);
    const events = ids.map(event);
    const seedTasks = ids.map((id) => task(id, `old-${id}`));
    let request = 0;
    fetchByIds.mockImplementation(async (_groupId, batch) => {
      request += 1;
      if (request === 2) {
        return { ok: false, error: { code: "temporary", message: "retry" } };
      }
      return {
        ok: true,
        result: { tasks: batch.map((id) => task(id, `new-${id}`)), tasksVersion: "tasksv:2" },
      };
    });

    await act(async () => root.render(<Probe events={events} seedTasks={seedTasks} />));

    expect(fetchByIds).toHaveBeenCalledTimes(2);
    expect(host.textContent).toBe("new-T001|old-T101");

    await act(async () => vi.advanceTimersByTimeAsync(1_000));

    expect(fetchByIds).toHaveBeenCalledTimes(4);
    expect(host.textContent).toBe("new-T001|new-T101");
  });

  it("does not abort an in-flight task lookup for unrelated ledger events", async () => {
    const seedTasks: Task[] = [];
    let resolveRequest:
      | ((value: Awaited<ReturnType<typeof api.fetchTasksByIds>>) => void)
      | undefined;
    fetchByIds.mockImplementation(
      (_groupId, batch) =>
        new Promise((resolve) => {
          resolveRequest = resolve;
          expect(batch).toEqual(["T001"]);
        }),
    );

    await act(async () => root.render(<Probe events={[event("T001")]} seedTasks={seedTasks} />));
    const signal = fetchByIds.mock.calls[0]?.[2];
    expect(fetchByIds).toHaveBeenCalledTimes(1);
    expect(signal?.aborted).toBe(false);

    const unrelated = { data: { text: "unrelated chat" } } as LedgerEvent;
    await act(async () =>
      root.render(<Probe events={[event("T001"), unrelated]} seedTasks={seedTasks} />),
    );

    expect(fetchByIds).toHaveBeenCalledTimes(1);
    expect(signal?.aborted).toBe(false);

    await act(async () => {
      resolveRequest?.({
        ok: true,
        result: { tasks: [task("T001", "loaded")], tasksVersion: "tasksv:2" },
      });
      await Promise.resolve();
    });
    expect(host.textContent).toBe("loaded|undefined");
  });
});
