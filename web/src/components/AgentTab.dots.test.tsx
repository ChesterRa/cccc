// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, GroupDoc } from "../types";

const mocks = vi.hoisted(() => ({
  pause: vi.fn(),
  launch: vi.fn(),
  edit: vi.fn(),
  browser: vi.fn(),
  settings: vi.fn(),
  load: vi.fn(),
  recovery: vi.fn(),
  update: vi.fn(),
  access: true,
  settingsOpen: false,
  groupDoc: null as GroupDoc | null,
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
  useGroupStore: (select: (state: unknown) => unknown) =>
    select({ chatByGroup: {}, groupDoc: mocks.groupDoc }),
  useUIStore: (select: (state: unknown) => unknown) =>
    select({ actorBusy: {}, canAccessGlobalSettings: mocks.access }),
  useObservabilityStore: (select: (state: unknown) => unknown) =>
    select({ loaded: true, terminalScrollbackLines: 1000 }),
  useTerminalSignalsStore: (select: (state: unknown) => unknown) => select({}),
  useModalStore: (select: (state: unknown) => unknown) =>
    select({
      openActorEditor: mocks.edit,
      openSettingsTarget: mocks.settings,
      modals: { settings: mocks.settingsOpen },
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
vi.mock("../services/api/localSessions", () => ({
  fetchLocalSessionRecovery: (...args: unknown[]) => mocks.recovery(...args),
}));
vi.mock("../services/api", () => ({
  fetchWebModelConnectors: async () => ({
    ok: true,
    result: {
      connectors: [{ connector_id: "shared", provider: "chatgpt_web" }],
      connector_base_url: "https://cccc.example.test",
    },
  }),
  fetchGrokActorState: (...args: unknown[]) => mocks.load(...args),
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
  mocks.settingsOpen = false;
  mocks.groupDoc = { group_id: "g_dot", state: "active" };
  vi.useFakeTimers();
  mocks.load.mockResolvedValue(dotsState());
  mocks.recovery.mockImplementation(async (groupId: string, actorId: string) => ({
    ok: true,
    result: {
      group_id: groupId,
      actor_id: actorId,
      source_id: "codex:fixture",
      recovery: {
        state: "connected",
        cwd: "/synthetic/native",
        command: "CCCC_HOME='/synthetic/cccc' codex resume 'fixture'",
      },
    },
  }));
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
async function render(
  compact: boolean,
  currentActor = actor,
  groupId = "g_dot",
  readOnly = false,
  isVisible = true,
) {
  await act(async () =>
    root.render(
      <AgentTab
        actor={currentActor}
        groupId={groupId}
        agentState={null}
        isVisible={isVisible}
        compact={compact}
        readOnly={readOnly}
        actorStatusProvisional={false}
        onQuit={mocks.pause}
        onLaunch={mocks.launch}
        onRelaunch={vi.fn()}
        onNewSession={vi.fn()}
        onEdit={mocks.edit}
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
    webModelProvider: "chatgpt_web",
    webModelSection: "connector",
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

it.each(["working", "interrupted"])(
  "keeps one Grok access control in the detail footer during %s work",
  async (phase) => {
    mocks.load.mockResolvedValue({
      ok: true,
      result: {
        group_id: "g_dot",
        actor_id: "bot",
        connected: true,
        bound: true,
        configured: true,
        enabled: true,
        phase,
        pending_count: 0,
        active_turn_id: "unfinished-turn",
      },
    });
    await render(false, { ...actor, id: "bot", runtime: "grok_web_model" });
    const controls = [...host.querySelectorAll<HTMLButtonElement>("button")].filter(
      (button) =>
        button.getAttribute("aria-label") === "dotsPause" ||
        button.textContent === "actors:dotsPause",
    );
    expect(controls).toHaveLength(1);
    expect(controls[0].getAttribute("aria-label")).toBe("dotsPause");
    await act(async () => controls[0].click());
    expect(mocks.pause).toHaveBeenCalledOnce();
    expect(mocks.update).not.toHaveBeenCalled();
  },
);

it("renders a Grok native pane and task status without a terminal or browser", async () => {
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      group_id: "g_dot",
      actor_id: "bot",
      connected: true,
      bound: true,
      configured: true,
      enabled: true,
      phase: "working",
      pending_count: 2,
    },
  });
  const grok = { ...actor, id: "bot", runtime: "grok_web_model" } as Actor;
  await render(true, grok);
  expect(mocks.browser).not.toHaveBeenCalled();
  expect(host.textContent).toContain("settings:grokWebhook.phase.working");
  expect(host.textContent).toContain("grokActor.configured");
  expect(host.querySelector("select")).toBeNull();
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((b) => b.textContent === "grokActor.connectionSettings")!
      .click(),
  );
  expect(mocks.edit).toHaveBeenCalledWith(grok, "chatgpt");
});

it("uses the footer's single Actor editor entry for a configured Dots pane", async () => {
  mocks.load.mockResolvedValue(
    dotsState({ connected: true, paired: true, subscribed: true, phase: "ready" }),
  );
  await render(false);
  expect(host.textContent).not.toContain("dotsActor.connectionSettings");
  expect(host.querySelector('section[aria-label="dotsActor.connectionSettings"]')).toBeNull();
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="editAgentConfig"]')!.click(),
  );
  expect(mocks.edit).toHaveBeenCalledOnce();
  expect(mocks.update).not.toHaveBeenCalled();
});

it("uses CCCC access controls and a shared-login destination for the browser runtime", async () => {
  const browserActor = { ...actor, runtime: "web_model", running: false } as Actor;
  await render(false, browserActor);
  expect(host.querySelector('[aria-label="dotsPause"]')).not.toBeNull();
  expect(host.querySelector('[aria-label="quitAgent"]')).toBeNull();
  expect(host.querySelector('[aria-label="sendInterruptLabel"]')).toBeNull();
  expect(host.querySelector('[aria-label="relaunchAgent"]')).toBeNull();
  expect(host.textContent).toContain("webAccessEnabled");
  const props = mocks.browser.mock.calls.at(-1)![0];
  await act(async () => props.onOpenSharedSettings());
  expect(mocks.settings).toHaveBeenCalledWith({
    scope: "global",
    tab: "webModels",
    webModelProvider: "chatgpt_web",
    webModelSection: "login",
  });
});
it("does not fetch restricted Grok state for a Group writer", async () => {
  mocks.access = false;
  await render(false, { ...actor, runtime: "grok_web_model", running: true } as Actor);
  expect(mocks.load).not.toHaveBeenCalled();
  expect(mocks.browser).not.toHaveBeenCalled();
  expect(host.textContent).toContain("grokActor.adminManaged");
  expect(host.querySelector('button[aria-label="dotsPause"]')).not.toBeNull();
});

it("does not carry a Dot's state into Grok after a runtime change", async () => {
  mocks.load.mockResolvedValue(dotsState({ connected: true, subscribed: true, phase: "ready" }));
  await render(true);
  let finish!: (value: unknown) => void;
  mocks.load.mockReturnValueOnce(
    new Promise((r) => {
      finish = r;
    }),
  );
  await render(true, { ...actor, runtime: "grok_web_model" } as Actor);
  expect(host.querySelector("[data-terminal-title-bar]")?.textContent).not.toContain(
    "settings:grokWebhook.phase.ready",
  );
  await act(async () =>
    finish({
      ok: true,
      result: {
        group_id: "g_dot",
        actor_id: "dot",
        connected: true,
        bound: false,
        configured: false,
        enabled: true,
        phase: "unconfigured",
        pending_count: 0,
      },
    }),
  );
  expect(host.querySelector("[data-terminal-title-bar]")?.textContent).toContain(
    "settings:grokWebhook.phase.unconfigured",
  );
});

it("passes the same instance-connection permission to the ChatGPT browser panel", async () => {
  mocks.access = false;
  await render(false, { ...actor, runtime: "web_model" });
  expect(mocks.browser).toHaveBeenCalled();
  expect(mocks.browser.mock.calls.at(-1)?.[0].canAccessConnection).toBe(false);
});

const nativeActor: Actor = {
  id: "native",
  runtime: "codex",
  external_session_id: "codex:fixture",
  enabled: true,
  running: false,
  runner_effective: "headless",
  runtime_state_source: "external_session",
  effective_working_state: "waiting",
};

it.each([false, true])(
  "distinguishes native access, Group lifecycle and actual receive availability (compact=%s)",
  async (compact) => {
    const cases = [
      ["active", false, false, "webAccessPaused"],
      ["active", false, true, "webAccessPaused"],
      ["paused", true, false, "nativeSession.groupPaused"],
      ["paused", true, true, "nativeSession.groupPaused"],
      ["stopped", true, false, "nativeSession.groupStopped"],
      ["stopped", true, true, "nativeSession.groupStopped"],
      ["active", true, false, "nativeSession.unavailable"],
      ["idle", true, false, "nativeSession.unavailable"],
      ["idle", true, true, "nativeSession.available"],
      ["active", true, true, "nativeSession.available"],
    ] as const;
    for (const [state, enabled, running, status] of cases) {
      mocks.groupDoc = { group_id: "g_dot", state };
      await render(compact, { ...nativeActor, enabled, running });
      expect(host.textContent).toContain(status);
      expect(host.querySelector('[role="status"]')?.textContent).toBe(status);
      if (status !== "nativeSession.unavailable")
        expect(host.textContent).not.toContain("nativeSession.unavailable");
      if (compact) {
        const header = host.querySelector("[data-terminal-title-bar]")!;
        expect(header.textContent).toContain(status);
        const indicator = header.querySelector('[role="img"]')!;
        expect(indicator.getAttribute("aria-label")).toBe(status);
        expect(indicator.className).toContain(
          status === "nativeSession.available" ? "bg-emerald-500" : "bg-slate-400/65",
        );
      }
    }
    expect(mocks.load).not.toHaveBeenCalled();
    expect(mocks.browser).not.toHaveBeenCalled();
    expect(host.querySelector(".xterm")).toBeNull();
  },
);

it.each([false, true])(
  "does not diagnose native failure from a missing or different Group snapshot (compact=%s)",
  async (compact) => {
    for (const groupDoc of [
      null,
      { group_id: "g_other", state: "paused" } as GroupDoc,
      { group_id: "g_dot" },
    ]) {
      mocks.groupDoc = groupDoc;
      await render(compact, nativeActor);
      expect(host.querySelector('[role="status"]')?.textContent).toBe("common:loading");
      expect(host.textContent).not.toContain("nativeSession.unavailable");
      expect(host.textContent).not.toContain("nativeSession.groupPaused");
    }
  },
);

it.each([false, true])(
  "reads private native recovery only for a visible, writable administrator outside settings (compact=%s)",
  async (compact) => {
    const pausedActor = { ...nativeActor, enabled: false };
    await render(compact, pausedActor, "g_dot", false, false);
    expect(mocks.recovery).not.toHaveBeenCalled();
    mocks.settingsOpen = true;
    await render(compact, pausedActor);
    expect(mocks.recovery).not.toHaveBeenCalled();
    mocks.settingsOpen = false;
    mocks.access = false;
    await render(compact, pausedActor);
    expect(mocks.recovery).not.toHaveBeenCalled();
    mocks.access = true;
    await render(compact, pausedActor, "g_dot", true);
    expect(mocks.recovery).not.toHaveBeenCalled();

    await render(compact, pausedActor);
    expect(mocks.recovery).toHaveBeenCalledExactlyOnceWith("g_dot", "native");
    expect(host.querySelector('[role="status"]')?.textContent).toBe("webAccessPaused");
    expect(host.textContent).toContain("nativeSession.recovery.states.connected");
    expect(host.querySelector("pre code")?.textContent).toBe(
      "CCCC_HOME='/synthetic/cccc' codex resume 'fixture'",
    );
    const copy = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "nativeSession.recovery.copyCommand",
    )!;
    expect(copy.disabled).toBe(false);

    mocks.settingsOpen = true;
    await render(compact, pausedActor);
    expect(host.querySelector("pre code")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(9000));
    expect(mocks.recovery).toHaveBeenCalledOnce();
    mocks.settingsOpen = false;
    await render(compact, pausedActor);
    expect(mocks.recovery).toHaveBeenCalledTimes(2);
    await render(compact, pausedActor, "g_dot", true);
    expect(host.querySelector("pre code")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(9000));
    expect(mocks.recovery).toHaveBeenCalledTimes(2);
  },
);

it.each([false, true])(
  "resumes native membership without fake runtime controls and preserves read-only gating (compact=%s)",
  async (compact) => {
    mocks.access = false;
    await render(compact, { ...nativeActor, enabled: false });
    const resume = host.querySelector<HTMLButtonElement>('button[aria-label="dotsResume"]')!;
    expect(resume).not.toBeNull();
    expect(host.querySelector('button[aria-label="dotsEnable"]')).toBeNull();
    await act(async () => resume.click());
    expect(mocks.launch).toHaveBeenCalledOnce();
    await render(compact, nativeActor);
    if (compact) {
      await act(async () =>
        host.querySelector<HTMLButtonElement>('button[aria-label="actorControls"]')!.click(),
      );
      const pause = [...document.querySelectorAll("button")].find(
        (button) => button.textContent === "dotsPause",
      )!;
      expect(document.body.textContent).not.toContain("terminalHistory");
      expect(document.body.textContent).not.toContain("relaunch");
      expect(document.body.textContent).not.toContain("newSession");
      await act(async () => pause.click());
    } else {
      await act(async () =>
        host.querySelector<HTMLButtonElement>('button[aria-label="dotsPause"]')!.click(),
      );
      for (const label of [
        "newSessionAgent",
        "relaunchAgent",
        "sendInterruptLabel",
        "terminalHistory",
      ]) {
        expect(host.querySelector(`button[aria-label="${label}"]`)).toBeNull();
      }
    }
    expect(mocks.pause).toHaveBeenCalledOnce();
    expect(mocks.load).not.toHaveBeenCalled();
    expect(mocks.update).not.toHaveBeenCalled();
    await render(compact, nativeActor, "g_dot", true);
    for (const label of [
      "dotsPause",
      "dotsResume",
      "editAgentConfig",
      "removeAgent",
      "actorControls",
    ]) {
      expect(host.querySelector(`button[aria-label="${label}"]`)).toBeNull();
    }
  },
);
