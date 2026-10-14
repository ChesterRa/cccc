// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../types";

const mocks = vi.hoisted(() => ({
  pause: vi.fn(),
  launch: vi.fn(),
  edit: vi.fn(),
  browser: vi.fn(),
  settings: vi.fn(),
  load: vi.fn(),
  update: vi.fn(),
  access: true,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    constructor() {
      throw new Error("Dots must not create a native terminal");
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("../stores", () => ({
  useGroupStore: (select: (state: unknown) => unknown) => select({ chatByGroup: {} }),
  useUIStore: (select: (state: unknown) => unknown) =>
    select({ actorBusy: {}, canAccessGlobalSettings: mocks.access }),
  useObservabilityStore: (select: (state: unknown) => unknown) =>
    select({ loaded: true, terminalScrollbackLines: 1000 }),
  useTerminalSignalsStore: (select: (state: unknown) => unknown) => select({}),
  useModalStore: (select: (state: unknown) => unknown) =>
    select({
      openActorEditor: mocks.edit,
      openSettingsTarget: mocks.settings,
      modals: { settings: false },
    }),
}));
vi.mock("../hooks/useActorDisplayState", () => ({
  useActorDisplayState: ({ actor }: { actor: Actor }) => ({
    isRunning: actor.running,
    workingState: actor.effective_working_state,
  }),
}));
vi.mock("./agentTerminal/useAgentTerminalConnection", () => ({
  useAgentTerminalConnection: () => ({
    connectionStatus: "disconnected",
    terminalReady: false,
    terminalWritable: false,
    canSendInput: () => false,
  }),
}));
vi.mock("./webModel/WebModelRuntimePanel", () => ({ WebModelRuntimePanel: mocks.browser }));
vi.mock("./headless/ActorAcpControls", () => ({ ActorAcpControls: () => null }));
vi.mock("../services/api", () => ({
  fetchWebModelConnectors: async () => ({
    ok: true,
    result: {
      connectors: [{ connector_id: "shared", provider: "chatgpt_web" }],
      connector_base_url: "https://cccc.example.test",
    },
  }),
  fetchDotsActorState: (...args: unknown[]) => mocks.load(...args),
  updateDotsActorConnection: (...args: unknown[]) => mocks.update(...args),
}));
import { AgentTab } from "./AgentTab";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const actor = {
  id: "dot",
  runtime: "chatgpt_dots",
  enabled: true,
  running: false,
  runner_effective: "headless",
  runtime_state_source: "terminal",
  effective_working_state: "stopped",
} as Actor;
const dotsState = (patch = {}) => ({
  ok: true,
  result: {
    group_id: "g_dot",
    actor_id: "dot",
    enabled: true,
    connected: false,
    subscribed: false,
    phase: "unconfigured",
    pending_count: 0,
    ...patch,
  },
});
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  mocks.access = true;
  vi.useFakeTimers();
  mocks.load.mockResolvedValue(dotsState());
  mocks.update.mockResolvedValue({ ok: true, result: { instructions: "native setup only" } });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
async function render(compact: boolean, currentActor = actor, groupId = "g_dot") {
  await act(async () =>
    root.render(
      <AgentTab
        actor={currentActor}
        groupId={groupId}
        agentState={null}
        isVisible
        compact={compact}
        actorStatusProvisional={false}
        onQuit={mocks.pause}
        onLaunch={mocks.launch}
        onRelaunch={vi.fn()}
        onNewSession={vi.fn()}
        onEdit={vi.fn()}
        onRemove={vi.fn()}
        onInbox={vi.fn()}
        busy=""
        isDark={false}
        isSmallScreen={false}
      />,
    ),
  );
}

it.each([false, true])(
  "shows a GET-projected managed turn error without offering Start (compact=%s)",
  async (compact) => {
    const managedActor: Actor = {
      id: "managed",
      runtime: "pi",
      enabled: true,
      running: true,
      runner_effective: "headless",
      runtime_state_source: "managed_session",
      effective_working_state: "error",
      effective_working_reason: "Synthetic model rejection",
    };
    await render(compact, managedActor, "g_managed");
    expect(host.textContent).toContain("common:error");
    expect(host.querySelector('button[aria-label="launchAgentLabel"]')).toBeNull();
    if (compact) {
      expect(host.querySelector('[role="img"][aria-label="common:error"]')).not.toBeNull();
    } else {
      const pause = host.querySelector<HTMLButtonElement>('button[aria-label="quitAgent"]')!;
      expect(pause).not.toBeNull();
      await act(async () => pause.click());
      expect(mocks.pause).toHaveBeenCalledOnce();
    }
    await render(
      compact,
      {
        ...managedActor,
        effective_working_state: "working",
        effective_working_reason: "headless_turn_active",
      },
      "g_managed",
    );
    expect(host.textContent).not.toContain("common:error");
  },
);
it("keeps access pause reachable for an enabled Dots Actor in a stopped Group without changing actual status", async () => {
  await render(false);
  expect(host.textContent).toContain("dotsActor.phases.unconfigured");
  expect(mocks.browser).not.toHaveBeenCalled();
  const pause = host.querySelector<HTMLButtonElement>('button[aria-label="dotsPause"]')!;
  expect(pause).not.toBeNull();
  expect(host.querySelector('button[aria-label="launchAgentLabel"]')).toBeNull();
  expect(host.querySelector('button[aria-label="newSessionAgent"]')).toBeNull();
  await act(async () => pause.click());
  expect(mocks.pause).toHaveBeenCalledOnce();
  expect(mocks.launch).not.toHaveBeenCalled();
});
it("offers the same local pause in the compact Actor controls", async () => {
  await render(true);
  await act(async () =>
    host.querySelector<HTMLButtonElement>('button[aria-label="actorControls"]')!.click(),
  );
  const pause = [...document.querySelectorAll("button")].find(
    (button) => button.textContent === "dotsPause",
  )!;
  expect(pause).toBeDefined();
  await act(async () => pause.click());
  expect(mocks.pause).toHaveBeenCalledOnce();
});

it("uses the runtime pane's authoritative Dots phase in its header instead of generic Waiting", async () => {
  await render(false, { ...actor, running: true, effective_working_state: "waiting" });
  const header = host.querySelector('[id="runtime-inspector-g_dot-dot"]')!.parentElement!
    .parentElement!.parentElement!;
  expect(header.textContent).toContain("settings:dotsActor.phases.unconfigured");
  expect(header.textContent).not.toContain(" • waiting");
  expect(host.textContent).not.toContain("noAgentStateYet");
  expect(mocks.load).toHaveBeenCalledOnce();
  const connect = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "dotsActor.connect",
  )!;
  expect(connect.disabled).toBe(false);
  await act(async () => connect.click());
  expect(mocks.update).toHaveBeenCalledWith("g_dot", "dot", "connect", undefined);
  expect(mocks.edit).not.toHaveBeenCalled();
});

it("opens the shared ChatGPT settings directly without Actor editing", async () => {
  await render(false);
  const shared = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "dotsActor.sharedSettings",
  )!;
  expect(shared).toBeDefined();
  await act(async () => shared.click());
  expect(mocks.settings).toHaveBeenCalledWith({
    scope: "global",
    tab: "webModels",
    webModelProvider: "dots",
  });
  expect(mocks.edit).not.toHaveBeenCalled();
});

it("does not carry a previous Group's ready phase into the new Actor header while its read is pending", async () => {
  mocks.load.mockResolvedValue(dotsState({ connected: true, subscribed: true, phase: "ready" }));
  await render(true, { ...actor, running: true, effective_working_state: "waiting" });
  expect(host.querySelector("[data-terminal-title-bar]")?.textContent).toContain(
    "settings:dotsActor.phases.ready",
  );
  let finish!: (value: ReturnType<typeof dotsState>) => void;
  mocks.load.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render(
    true,
    { ...actor, id: "dot-b", running: true, effective_working_state: "waiting" },
    "g_b",
  );
  expect(host.querySelector("[data-terminal-title-bar]")?.textContent).not.toContain(
    "settings:dotsActor.phases.ready",
  );
  expect(host.querySelector("[data-terminal-title-bar]")?.textContent).not.toContain("waiting");
  await act(async () => finish(dotsState({ group_id: "g_b", actor_id: "dot-b" })));
  expect(host.querySelector("[data-terminal-title-bar]")?.textContent).toContain(
    "settings:dotsActor.phases.unconfigured",
  );
});

it("does not show a running indicator for an unpaired Dot with a valid subscription", async () => {
  mocks.load.mockResolvedValue(
    dotsState({ connected: true, paired: false, subscribed: true, phase: "waiting_pairing" }),
  );
  await render(true, { ...actor, running: true, effective_working_state: "waiting" });
  const indicator = host.querySelector(
    '[role="img"][aria-label="settings:dotsActor.phases.waiting_pairing"]',
  );
  expect(indicator).not.toBeNull();
  expect(indicator?.className).toContain("bg-slate-400/65");
  expect(indicator?.className).not.toContain("bg-emerald-500 ring-");
});

it("retains the working indicator for an accepted task after its event subscription expires", async () => {
  mocks.load.mockResolvedValue(
    dotsState({
      connected: true,
      subscribed: false,
      phase: "working",
      active_turn_id: "accepted-turn",
      subscription_expires_at: Date.now() - 1_000,
    }),
  );
  await render(true, { ...actor, running: true, effective_working_state: "waiting" });
  const indicator = host.querySelector(
    '[role="img"][aria-label="settings:dotsActor.phases.working"]',
  );
  expect(indicator).not.toBeNull();
  expect(indicator?.className).toContain("bg-emerald-500 ring-");
  expect(indicator?.className).not.toContain("bg-slate-400/65");
});

it("shows an administrator-managed connection for a Group writer without fetching restricted Dots state or losing local Pause", async () => {
  mocks.access = false;
  await render(false, { ...actor, running: true, effective_working_state: "waiting" });
  expect(host.textContent).toContain("settings:dotsActor.adminStatus");
  expect(host.textContent).toContain("dotsActor.adminManaged");
  expect(mocks.load).not.toHaveBeenCalled();
  expect(mocks.update).not.toHaveBeenCalled();
  expect(host.querySelector('button[aria-label="dotsPause"]')).not.toBeNull();
  expect(host.textContent).not.toContain(" • waiting");
  await act(async () => vi.advanceTimersByTimeAsync(4_000));
  expect(mocks.load).not.toHaveBeenCalled();
});
