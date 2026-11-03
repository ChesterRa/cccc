// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import type { Actor, LedgerEvent } from "../types";

const observed = vi.hoisted(() => ({ panels: [] as Array<{ actorId: string; texts: string[] }> }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("../hooks/useActorDisplayState", () => ({
  useActorDisplayState: () => ({ isRunning: true, workingState: "working" }),
}));
vi.mock("./agentTerminal/useAgentTerminalConnection", () => ({
  useAgentTerminalConnection: () => ({
    connectionStatus: "disconnected",
    terminalReady: false,
    terminalWritable: false,
    canSendInput: () => false,
  }),
}));
vi.mock("./headless/ActorAcpControls", () => ({ ActorAcpControls: () => null }));
vi.mock("./headless/HeadlessRuntimePanel", () => ({
  HeadlessRuntimePanel: (props: { actorId: string; streamingEvents?: LedgerEvent[] }) => {
    observed.panels.push({
      actorId: props.actorId,
      texts: (props.streamingEvents || []).map((event) =>
        String((event.data as { text?: string }).text || ""),
      ),
    });
    return <span data-panel={props.actorId} />;
  },
}));
import { AgentTab } from "./AgentTab";
import { useGroupStore, useObservabilityStore } from "../stores";

const actor = (id: string): Actor => ({
  id,
  runtime: "pi",
  enabled: true,
  running: true,
  runner_effective: "headless",
  runtime_state_source: "managed_session",
  effective_working_state: "working",
});
const event = (id: string, text: string): LedgerEvent => ({
  id: `stream:${id}`,
  kind: "chat.message",
  group_id: "g",
  by: id,
  ts: "2026-01-01T00:00:01Z",
  _streaming: true,
  data: { stream_id: `stream:${id}`, text, _headless_text_started_at: "2026-01-01T00:00:01Z" },
});
const noop = () => {};
const count = (id: string) => observed.panels.filter((call) => call.actorId === id).length;
let host: HTMLDivElement | undefined;
let root: ReturnType<typeof createRoot> | undefined;
const initial = useGroupStore.getState();
const initialObservability = useObservabilityStore.getState();
afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = undefined;
  host = undefined;
  useGroupStore.setState({
    chatByGroup: initial.chatByGroup,
    selectedGroupId: initial.selectedGroupId,
  });
  observed.panels.length = 0;
  useObservabilityStore.setState({ loaded: initialObservability.loaded });
  vi.unstubAllGlobals();
});

it("does not rebuild another actor's headless panel when only this actor's stream changes", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const fetchSpy = vi.fn(async () => new Response(JSON.stringify({ ok: true, result: {} })));
  vi.stubGlobal("fetch", fetchSpy);
  useObservabilityStore.setState({ loaded: true });
  useGroupStore.setState({ chatByGroup: {}, selectedGroupId: "g" });
  await act(async () => {
    useGroupStore.getState().upsertStreamingEvent(event("a", "A first"), "g");
    useGroupStore.getState().upsertStreamingEvent(event("b", "B first"), "g");
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const a = actor("a"),
    b = actor("b");
  const props = {
    groupId: "g",
    agentState: null,
    isVisible: true,
    actorStatusProvisional: false,
    onQuit: noop,
    onLaunch: noop,
    onRelaunch: noop,
    onNewSession: noop,
    onEdit: noop,
    onRemove: noop,
    onInbox: noop,
    busy: "",
    isDark: false,
    isSmallScreen: false,
  };
  await act(async () =>
    root!.render(
      <>
        <AgentTab {...props} actor={a} />
        <AgentTab {...props} actor={b} />
      </>,
    ),
  );
  await act(async () => new Promise((resolve) => setTimeout(resolve, 30)));
  expect(count("a")).toBeGreaterThan(0);
  expect(count("b")).toBeGreaterThan(0);
  observed.panels.length = 0;
  await act(async () => useGroupStore.getState().upsertStreamingEvent(event("b", "B second"), "g"));
  expect(count("a")).toBe(0);
  expect(count("b")).toBeGreaterThan(0);
  observed.panels.length = 0;
  await act(async () => useGroupStore.getState().upsertStreamingEvent(event("a", "A second"), "g"));
  expect(count("a")).toBeGreaterThan(0);
  expect(
    observed.panels
      .slice()
      .reverse()
      .find((entry) => entry.actorId === "a")?.texts,
  ).toContain("A second");
  expect(count("b")).toBe(0);
  expect(fetchSpy).not.toHaveBeenCalled();
});
