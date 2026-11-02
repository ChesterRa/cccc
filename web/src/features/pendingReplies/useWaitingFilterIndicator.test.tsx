// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vite-plus/test";
import { useUIStore } from "../../stores/useUIStore";
import { PendingSummaryContext } from "./context";
import type { PendingIndicator } from "./indicator";
import type { Summary } from "./usePendingSummary";
import { useWaitingFilterIndicator } from "./useWaitingFilterIndicator";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let seen: PendingIndicator | null = null;
function Probe({ groupId, active }: { groupId: string; active: boolean }) {
  seen = useWaitingFilterIndicator(groupId, active);
  return null;
}

async function read(summary: Summary | null, groupId = "g", active = false) {
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () =>
    root.render(
      summary ? (
        <PendingSummaryContext.Provider value={{ summaries: { g: summary }, refresh: () => {} }}>
          <Probe groupId={groupId} active={active} />
        </PendingSummaryContext.Provider>
      ) : (
        <Probe groupId={groupId} active={active} />
      ),
    ),
  );
  await act(async () => root.unmount());
  return seen;
}

afterEach(() => {
  useUIStore.setState({ showPendingPrompts: true });
  localStorage.removeItem("cccc-pending-prompts");
});

it("offers the Group's Waiting for you filter only while those prompts are shown", async () => {
  const waiting: Summary = { state: "fresh", pending: 2 };
  expect((await read(waiting))?.count).toBe(2);
  // An open filter stays visible after its last answer, until the user leaves it.
  expect((await read({ state: "fresh", pending: 0 }, "g", true))?.count).toBe(0);

  useUIStore.getState().setShowPendingPrompts(false);
  expect(await read(waiting)).toBeNull();
  expect(await read({ state: "fresh", pending: 0 }, "g", true)).toBeNull();
});

it("offers no filter without a pending summary or a Group", async () => {
  expect(await read(null)).toBeNull();
  expect(await read({ state: "fresh", pending: 2 }, "")).toBeNull();
});
