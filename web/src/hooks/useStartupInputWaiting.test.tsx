// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore } from "../stores";
import type { Actor } from "../types";
import { useStartupInputRefresh } from "./useStartupInputRefresh";
import {
  STARTUP_INPUT_WAIT_SHOWN_AFTER_MS,
  useStartupInputWaiting,
} from "./useStartupInputWaiting";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const shown: boolean[] = [];

function Probe({ actor }: { actor: Actor }) {
  shown.push(useStartupInputWaiting("g_probe", actor));
  return null;
}

function Refresh({ groupId }: { groupId: string }) {
  useStartupInputRefresh(groupId);
  return null;
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  shown.length = 0;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("shows a wait only after the delay and keeps that delay across a remount", async () => {
  const waiting = {
    id: "amp",
    running: true,
    runtime_startup_input: { generation: "2026-11-18T01:00:00.000001Z" },
  } as Actor;
  await act(async () => root.render(<Probe actor={waiting} />));
  expect(shown.at(-1)).toBe(false);
  await act(async () => {
    vi.advanceTimersByTime(STARTUP_INPUT_WAIT_SHOWN_AFTER_MS - 1);
  });
  expect(shown.at(-1)).toBe(false);
  await act(async () => {
    vi.advanceTimersByTime(1);
  });
  expect(shown.at(-1)).toBe(true);
  // Opening the view again does not restart the delay for the same terminal process.
  await act(async () => root.render(<></>));
  await act(async () => root.render(<Probe actor={waiting} />));
  expect(shown.at(-1)).toBe(true);
  // A stopped terminal or a new process is not this wait.
  await act(async () => root.render(<Probe actor={{ ...waiting, running: false }} />));
  expect(shown.at(-1)).toBe(false);
  await act(async () =>
    root.render(
      <Probe
        actor={{ ...waiting, runtime_startup_input: { generation: "2026-11-18T01:00:09.000001Z" } }}
      />,
    ),
  );
  expect(shown.at(-1)).toBe(false);
});

it("reads Actors again only while a member of the shown Group waits", async () => {
  const refreshActors = vi.fn(async () => {});
  const member = { id: "kimi", running: true } as Actor;
  useGroupStore.setState({
    selectedGroupId: "g_wait",
    actors: [{ ...member, runtime_startup_input: { generation: "2026-11-18T02:00:00.000001Z" } }],
    refreshActors,
  });
  await act(async () => root.render(<Refresh groupId="g_wait" />));
  await act(async () => {
    vi.advanceTimersByTime(3000);
  });
  expect(refreshActors).toHaveBeenCalledWith("g_wait", { includeUnread: false });
  const reads = refreshActors.mock.calls.length;
  await act(async () => useGroupStore.setState({ actors: [member] }));
  await act(async () => {
    vi.advanceTimersByTime(30000);
  });
  expect(refreshActors).toHaveBeenCalledTimes(reads);
});
