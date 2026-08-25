// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../src/types";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const apiMocks = vi.hoisted(() => ({ fetchTasksByIds: vi.fn() }));
vi.mock("../../src/services/api", () => apiMocks);

import {
  collectTaskReferenceIds,
  useTaskReferenceIndex,
} from "../../src/hooks/chat/useTaskReferenceIndex";

const EVENTS: LedgerEvent[] = [
  {
    kind: "chat.message",
    data: {
      refs: [
        { kind: "task_ref", task_id: "T002", title: "Historical title" },
        { kind: "task_ref", task_id: "T001" },
        { kind: "task_ref", task_id: "T002" },
      ],
    },
  },
];

function Probe({ version }: { version: string }) {
  const tasks = useTaskReferenceIndex({ groupId: "g1", events: EVENTS, tasksVersion: version });
  return <span>{tasks.get("T002")?.status || "missing"}</span>;
}

describe("useTaskReferenceIndex", () => {
  beforeEach(() => apiMocks.fetchTasksByIds.mockReset());

  it("deduplicates and sorts task references", () => {
    expect(collectTaskReferenceIds(EVENTS)).toEqual(["T001", "T002"]);
  });

  it("refreshes live chips and removes deleted tasks on task revision changes", async () => {
    let status = "active";
    apiMocks.fetchTasksByIds.mockImplementation((_groupId: string, ids: string[]) =>
      Promise.resolve({
        ok: true,
        result: {
          tasks: status ? ids.map((id) => ({ id, title: `Live ${id}`, status })) : [],
          tasksVersion: "ignored",
        },
      }),
    );
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe version="tasksv:1" />));
    await vi.waitFor(() => expect(host.textContent).toBe("active"));
    expect(apiMocks.fetchTasksByIds).toHaveBeenLastCalledWith(
      "g1",
      ["T001", "T002"],
      expect.any(AbortSignal),
    );

    status = "done";
    await act(async () => root.render(<Probe version="tasksv:2" />));
    await vi.waitFor(() => expect(host.textContent).toBe("done"));

    status = "";
    await act(async () => root.render(<Probe version="tasksv:3" />));
    await vi.waitFor(() => expect(host.textContent).toBe("missing"));
    expect(apiMocks.fetchTasksByIds).toHaveBeenCalledTimes(3);
    await act(async () => root.unmount());
  });
});
