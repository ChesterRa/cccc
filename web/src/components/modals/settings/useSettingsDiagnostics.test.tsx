// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { TFunction } from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useSettingsDiagnostics } from "./useSettingsDiagnostics";

const api = vi.hoisted(() => ({
  tail: vi.fn(),
  snapshot: vi.fn(),
  logs: vi.fn(),
  clearTail: vi.fn(),
  clearLogs: vi.fn(),
}));
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchActors: async (group: string) => ({
    ok: true,
    result: { actors: [{ id: `${group}-actor-1` }, { id: `${group}-actor-2` }] },
  }),
  fetchTerminalTail: api.tail,
  fetchDebugSnapshot: api.snapshot,
  fetchLogTail: api.logs,
  clearTerminalTail: api.clearTail,
  clearLogs: api.clearLogs,
}));

const t = ((key: string) => key) as TFunction<"settings">;
let current: ReturnType<typeof useSettingsDiagnostics>;
function Probe({ group = "A", open = true }: { group?: string; open?: boolean }) {
  current = useSettingsDiagnostics({
    isOpen: open,
    groupId: group,
    scope: "group",
    groupTab: "transcript",
    globalTab: "account",
    hasSettingsTarget: false,
    canAccessGlobalSettings: true,
    t,
  });
  return null;
}
function deferred() {
  let resolve!: (value: unknown) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
type Channel = "tail" | "snapshot" | "logs";
const channels: Channel[] = ["tail", "snapshot", "logs"];
const read = (channel: Channel) =>
  channel === "tail"
    ? current.loadTerminalTail()
    : channel === "snapshot"
      ? current.loadDebugSnapshot()
      : current.loadLogTail();
const text = (channel: Channel) =>
  channel === "tail"
    ? current.tailText
    : channel === "snapshot"
      ? current.debugSnapshot
      : current.logText;
const busy = (channel: Channel) =>
  channel === "tail"
    ? current.tailBusy
    : channel === "snapshot"
      ? current.debugSnapshotBusy
      : current.logBusy;
const error = (channel: Channel) =>
  channel === "tail"
    ? current.tailErr
    : channel === "snapshot"
      ? current.debugSnapshotErr
      : current.logErr;
const response = (channel: Channel, value: string) => ({
  ok: true,
  result:
    channel === "tail"
      ? { text: value, hint: `${value} hint` }
      : channel === "snapshot"
        ? { fixture: value }
        : { lines: [value] },
});

describe("settings diagnostics request ownership", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const render = (group = "A", open = true) =>
    act(async () => root.render(<Probe group={group} open={open} />));
  const start = async (channel: Channel) => {
    let pending!: Promise<void>;
    await act(async () => {
      pending = read(channel);
    });
    return { pending };
  };
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.resetAllMocks();
    for (const channel of channels)
      api[channel].mockResolvedValue(response(channel, "current result"));
    api.clearTail.mockResolvedValue({ ok: true, result: {} });
    api.clearLogs.mockResolvedValue({ ok: true, result: {} });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it.each(channels)("clears already displayed %s when the Group changes", async (channel) => {
    await render();
    await act(async () => read(channel));
    expect(text(channel)).toContain("current result");
    await render("B");
    expect(text(channel)).toBe("");
    expect(error(channel)).toBe("");
    expect(busy(channel)).toBe(false);
  });

  it.each(
    channels.flatMap((channel) =>
      ["success", "failure"].map((outcome) => [channel, outcome] as const),
    ),
  )("rejects A's late %s %s after switching to B", async (channel, outcome) => {
    await render();
    const old = deferred();
    api[channel].mockReturnValueOnce(old.promise);
    const { pending } = await start(channel);
    expect(busy(channel)).toBe(true);
    await render("B");
    expect(current.tailActorId).toBe("B-actor-1");
    expect(busy(channel)).toBe(false);
    await act(async () => {
      if (outcome === "success") old.resolve(response(channel, "A-only result"));
      else old.reject(new Error("A-only error"));
      await pending;
    });
    expect(text(channel)).toBe("");
    expect(error(channel)).toBe("");
    expect(busy(channel)).toBe(false);
  });

  it.each(channels)(
    "does not revive a departed A visit's %s after returning to A",
    async (channel) => {
      await render();
      const old = deferred();
      api[channel].mockReturnValueOnce(old.promise);
      const { pending } = await start(channel);
      await render("B");
      await render("A");
      await act(async () => {
        old.resolve(response(channel, "departed result"));
        await pending;
      });
      expect(text(channel)).toBe("");
      expect(busy(channel)).toBe(false);
    },
  );

  it.each(
    channels.flatMap((channel) =>
      ["old-first", "new-first"].map((order) => [channel, order] as const),
    ),
  )("keeps the newest %s read authoritative (%s)", async (channel, order) => {
    await render();
    const old = deferred(),
      latest = deferred();
    api[channel].mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
    const first = await start(channel),
      second = await start(channel);
    if (order === "old-first") {
      await act(async () => {
        old.resolve(response(channel, "old result"));
        await first.pending;
      });
      expect(text(channel)).toBe("");
      expect(busy(channel)).toBe(true);
      await act(async () => {
        latest.resolve(response(channel, "latest result"));
        await second.pending;
      });
    } else {
      await act(async () => {
        latest.resolve(response(channel, "latest result"));
        await second.pending;
      });
      await act(async () => {
        old.resolve(response(channel, "old result"));
        await first.pending;
      });
    }
    expect(text(channel)).toContain("latest result");
    expect(busy(channel)).toBe(false);
  });

  it.each(channels)(
    "does not let a closed modal's %s response affect its next opening",
    async (channel) => {
      await render();
      const old = deferred();
      api[channel].mockReturnValueOnce(old.promise);
      const { pending } = await start(channel);
      await render("A", false);
      expect(busy(channel)).toBe(false);
      await render("A");
      await act(async () => {
        old.resolve(response(channel, "closed visit"));
        await pending;
      });
      expect(text(channel)).toBe("");
      expect(busy(channel)).toBe(false);
    },
  );

  it("clears the previous Actor's transcript and rejects its pending read", async () => {
    await render();
    await act(async () => read("tail"));
    const old = deferred();
    api.tail.mockReturnValueOnce(old.promise);
    const { pending } = await start("tail");
    await act(async () => current.setTailActorId("A-actor-2"));
    expect(current.tailText).toBe("");
    expect(current.tailHint).toBe("");
    expect(current.tailBusy).toBe(false);
    await act(async () => read("tail"));
    await act(async () => {
      old.resolve(response("tail", "wrong Actor"));
      await pending;
    });
    expect(current.tailText).toBe("current result");
    expect(api.tail.mock.calls.at(-1)?.slice(0, 2)).toEqual(["A", "A-actor-2"]);
  });

  it("does not restore a snapshot after the user clears it during a read", async () => {
    await render();
    const old = deferred();
    api.snapshot.mockReturnValueOnce(old.promise);
    const { pending } = await start("snapshot");
    await act(async () => current.clearDebugSnapshot());
    expect(current.debugSnapshotBusy).toBe(false);
    await act(async () => {
      old.resolve(response("snapshot", "cleared result"));
      await pending;
    });
    expect(current.debugSnapshot).toBe("");
  });

  it("rejects logs from the previously selected component", async () => {
    await render();
    const old = deferred();
    api.logs.mockReturnValueOnce(old.promise);
    const { pending } = await start("logs");
    await act(async () => current.setLogComponent("im"));
    expect(current.logBusy).toBe(false);
    await act(async () => read("logs"));
    await act(async () => {
      old.resolve(response("logs", "old daemon log"));
      await pending;
    });
    expect(current.logText).toBe("current result");
    expect(api.logs.mock.calls.at(-1)?.slice(0, 2)).toEqual(["im", "A"]);
  });

  it.each(["tail", "logs"] as const)(
    "does not let A's completed %s clear erase B's result",
    async (channel) => {
      await render();
      await act(async () => current.setDeveloperMode(true));
      const old = deferred();
      (channel === "tail" ? api.clearTail : api.clearLogs).mockReturnValueOnce(old.promise);
      let pending!: Promise<void>;
      await act(async () => {
        pending = channel === "tail" ? current.clearTail() : current.handleClearLogs();
      });
      await render("B");
      await act(async () => read(channel));
      await act(async () => {
        old.resolve({ ok: true, result: {} });
        await pending;
      });
      expect(text(channel)).toBe("current result");
      expect(busy(channel)).toBe(false);
    },
  );
});
