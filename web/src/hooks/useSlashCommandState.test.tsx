// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../services/api";
import { publishCapabilityChanged } from "../utils/capabilityEvents";
import { useSlashCommandState } from "./useSlashCommandState";

vi.mock("../services/api", () => ({ fetchSlashCommandCapabilityState: vi.fn() }));

type StateResponse = Awaited<ReturnType<typeof api.fetchSlashCommandCapabilityState>>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function response(skillName: string): StateResponse {
  return {
    ok: true,
    result: {
      group_id: "g-test",
      actor_id: "user",
      enabled: [],
      active_capsule_skills: [
        {
          capability_id: `skill:test:${skillName}`,
          name: skillName,
          description_short: `${skillName} skill`,
        },
      ],
      dynamic_tools: [],
      actor_hidden_capabilities: [],
    },
  } as StateResponse;
}

function Probe({ groupId }: { groupId: string }) {
  const { slashCommands } = useSlashCommandState(groupId);
  return <div>{slashCommands.map((command) => command.command).join(",")}</div>;
}

describe("useSlashCommandState", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const fetchState = vi.mocked(api.fetchSlashCommandCapabilityState);

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    fetchState.mockReset();
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    window.localStorage.clear();
  });

  it("keeps the event-triggered catalog when an older initial request finishes last", async () => {
    const initial = deferred<StateResponse>();
    const refreshed = deferred<StateResponse>();
    fetchState.mockReturnValueOnce(initial.promise).mockReturnValueOnce(refreshed.promise);

    await act(async () => root.render(<Probe groupId="g-latest" />));
    expect(fetchState).toHaveBeenCalledTimes(1);

    await act(async () => publishCapabilityChanged("g-latest"));
    expect(fetchState).toHaveBeenCalledTimes(2);
    await act(async () => refreshed.resolve(response("fresh")));
    expect(host.textContent).toContain("/fresh");

    await act(async () => initial.resolve(response("stale")));
    expect(host.textContent).toContain("/fresh");
    expect(host.textContent).not.toContain("/stale");
  });

  it("keeps the last good catalog after refresh failure and recovers on the next event", async () => {
    fetchState
      .mockResolvedValueOnce(response("stable"))
      .mockRejectedValueOnce(new Error("refresh failed"))
      .mockResolvedValueOnce(response("recovered"));

    await act(async () => root.render(<Probe groupId="g-recovery" />));
    expect(host.textContent).toContain("/stable");

    await act(async () => publishCapabilityChanged("g-recovery"));
    expect(host.textContent).toContain("/stable");

    await act(async () => publishCapabilityChanged("g-recovery"));
    expect(host.textContent).toContain("/recovered");
    expect(host.textContent).not.toContain("/stable");
  });
});
