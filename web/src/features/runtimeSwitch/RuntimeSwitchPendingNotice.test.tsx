// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import * as actorApi from "../../services/api/actors";
import { useGroupStore, useUIStore } from "../../stores";
import * as switchState from "./runtimeSwitchState";
import { RuntimeSwitchPendingNotice } from "./RuntimeSwitchPendingNotice";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("reports the new runtime state after retrying a held switch", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const groupBefore = useGroupStore.getState();
  const uiBefore = useUIStore.getState();
  const notice = vi.fn();
  useGroupStore.setState({
    selectedGroupId: "g",
    groupDoc: {
      group_id: "g",
      runtime_switches: [{ actor_id: "worker", actor_generation: "generation-1", pending: true }],
    },
  });
  useUIStore.setState({ isWebAdmin: true, webReadOnly: false, showNotice: notice });
  const complete = vi
    .spyOn(actorApi, "completeActorRuntimeSwitch")
    .mockResolvedValue({
      ok: true,
      result: {
        group_id: "g",
        actor_id: "worker",
        op_id: "operation-1",
        switched: true,
        from: { kind: "native", runtime: "codex" },
        to: { kind: "managed", runtime: "claude" },
        completion: "done",
        start: "error",
        start_error: { code: "not_found", message: "Executable was not found" },
      },
    });
  const refresh = vi.spyOn(switchState, "refreshRuntimeSwitchGroup").mockResolvedValue(undefined);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () =>
      root.render(
        <RuntimeSwitchPendingNotice
          groupId="g"
          actor={{ id: "worker", generation: "generation-1" }}
        />,
      ),
    );
    const button = [...host.querySelectorAll("button")].find(
      (item) => item.textContent === "runtimeSwitch.retry",
    )!;
    await act(async () => button.click());
    expect(complete).toHaveBeenCalledExactlyOnceWith("g", "worker");
    expect(refresh).toHaveBeenCalledExactlyOnceWith("g");
    expect(notice).toHaveBeenCalledWith({
      message: "runtimeSwitch.startError Executable was not found",
    });
    for (const [start, message] of [
      ["pending", "runtimeSwitch.startPending"],
      ["stopped", "runtimeSwitch.stopped"],
    ] as const) {
      complete.mockResolvedValueOnce({
        ok: true,
        result: {
          group_id: "g",
          actor_id: "worker",
          op_id: "operation-1",
          switched: true,
          from: { kind: "native", runtime: "codex" },
          to: { kind: "managed", runtime: "claude" },
          completion: "done",
          start,
        },
      });
      await act(async () => button.click());
      expect(notice).toHaveBeenLastCalledWith({ message });
    }
  } finally {
    await act(async () => root.unmount());
    host.remove();
    useGroupStore.setState(groupBefore, true);
    useUIStore.setState(uiBefore, true);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
