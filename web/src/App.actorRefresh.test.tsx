// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, GroupDoc, GroupMeta } from "./types";
import type { ChatTab } from "./pages/chat";
import {
  useComposerStore,
  useGroupStore,
  useModalStore,
  useObservabilityStore,
  useUIStore,
} from "./stores";
import {
  deferredUnreadRefreshTimers,
  groupViewCache,
  loadGroupInFlight,
  refreshActorsInFlight,
  refreshActorsQueued,
} from "./stores/groupStoreCore";
import { useWorkspaceNavigation } from "./stores/workspaceNavigation";
import { clearAuthToken } from "./services/api/base";
import i18n, { i18nReady } from "./i18n";
import enActors from "./i18n/locales/en/actors.json";

const noop = vi.hoisted(() => () => {});
// Keep App/AppShell/ActorTab/AgentTab, display-state authority, stores and HTTP
// helpers real. Only unrelated startup, voice, chat layout and terminal I/O
// are replaced; mocking refreshActors or onStatusChange would hide the loop.
vi.mock("./hooks/useAppChrome", () => ({
  useAppChrome: () => ({
    canManageGroups: true,
    ccccHome: "",
    fetchDirSuggestions: noop,
    refreshWebAccessSession: async () => true,
  }),
}));
vi.mock("./hooks/useSSE", () => ({
  useSSE: () => ({ connectStream: noop, fetchContext: noop, cleanup: noop }),
}));
vi.mock("./hooks/useGlobalEvents", () => ({ useGlobalEvents: noop }));
vi.mock("./features/connect/useConnectWorkbench", () => ({
  useConnectWorkbench: () => ({ selected: null, selectLocal: noop, accountLabel: null }),
}));
vi.mock("./features/connect/GroupConnectionsControl", () => ({
  GroupConnectionsControl: () => null,
}));
vi.mock("./features/codexVoice/useCodexVoiceShell", () => ({
  useCodexVoiceShell: () => ({ controller: { isEngaged: false } }),
}));
vi.mock("./features/codexVoice/CodexVoiceShellSurfaces", () => ({
  CodexVoiceMobileDock: () => null,
  CodexVoiceOverlays: () => null,
}));
vi.mock("./features/codexVoice/useVoiceViewedMessages", () => ({ useVoiceViewedMessages: noop }));
vi.mock("./components/layout/GroupSidebar", () => ({
  GroupSidebar: ({
    orderedGroups,
    onSelectGroup,
  }: {
    orderedGroups: GroupMeta[];
    onSelectGroup: (groupId: string) => void;
  }) => (
    <nav aria-label="Groups">
      {orderedGroups.map((group) => (
        <button key={group.group_id} onClick={() => onSelectGroup(group.group_id)}>
          {group.title}
        </button>
      ))}
    </nav>
  ),
}));
vi.mock("./components/layout/AppHeader", () => ({
  AppHeader: ({ onThemeChange }: { onThemeChange: (theme: "light") => void }) => (
    <button onClick={() => onThemeChange("light")}>Light theme</button>
  ),
}));
vi.mock("./pages/chat", () => ({
  ChatTab: ({
    selectedGroupId,
    runtimeActors,
    activeRuntimeActorId,
    onOpenRuntimeActor,
    renderRuntimeActor,
  }: ComponentProps<typeof ChatTab>) => (
    <div>
      <button onClick={() => onOpenRuntimeActor?.("chat")}>Messages</button>
      {runtimeActors?.map((actor) => (
        <button key={actor.id} onClick={() => onOpenRuntimeActor?.(actor.id)}>
          Open {actor.title}
        </button>
      ))}
      {runtimeActors?.map((actor) => (
        <article
          key={`${selectedGroupId}/${actor.id}`}
          aria-label={`${selectedGroupId}/${actor.id}`}
          hidden={activeRuntimeActorId !== actor.id}
        >
          {renderRuntimeActor?.(actor.id, {
            isVisible: activeRuntimeActorId === actor.id,
            compact: false,
            onExpand: noop,
          })}
        </article>
      ))}
    </div>
  ),
}));
vi.mock("./components/agentTerminal/useAgentTerminalConnection", () => ({
  useAgentTerminalConnection: () => ({
    connectionStatus: "disconnected",
    terminalReady: false,
    terminalWritable: false,
    canSendInput: () => false,
  }),
}));
vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("./components/headless/HeadlessRuntimePanel", () => ({ HeadlessRuntimePanel: () => null }));
vi.mock("./components/headless/ActorAcpControls", () => ({ ActorAcpControls: () => null }));
import App from "./App";

const GROUP_A = "g_reconcile_alpha";
const GROUP_B = "g_reconcile_beta";
type ActorKind = "native" | "managed";
type RequestRecord = { method: string; pathname: string; at: number };
let host: HTMLDivElement;
let root: Root;
let requests: RequestRecord[];
let actorsByGroup: Record<string, Actor[]>;
let nextActorRead: { groupId: string; result: Promise<Actor[]> } | null;
let releasePendingReads: (() => void)[];
let unexpectedRequests: string[];

function actorFixture(kind: ActorKind, title = "Alpha worker"): Actor {
  return {
    id: "worker",
    title,
    role: "foreman",
    runtime: "codex",
    runner_effective: "headless",
    enabled: true,
    running: false,
    generation: `generation-${title}`,
    ...(kind === "native" ? { external_session_id: `codex:${title}` } : {}),
  };
}

function groupFixture(groupId: string): GroupDoc & GroupMeta {
  return {
    group_id: groupId,
    title: groupId === GROUP_A ? "Alpha Group" : "Beta Group",
    state: "active",
    running: actorsByGroup[groupId].some((actor) => actor.running),
  };
}

function jsonResponse(result: unknown): Response {
  return new Response(JSON.stringify({ ok: true, result }), {
    headers: { "Content-Type": "application/json" },
  });
}

async function serveRequest(input: string | URL | Request, init?: RequestInit): Promise<Response> {
  const url = new URL(
    typeof input === "string" ? input : input instanceof URL ? input.href : input.url,
    "http://cccc.test",
  );
  const method = init?.method || "GET";
  requests.push({ method, pathname: url.pathname, at: Date.now() });
  if (method !== "GET") {
    unexpectedRequests.push(`${method} ${url.pathname}`);
    throw new Error(`Unexpected mutation: ${method} ${url.pathname}`);
  }
  const groupMatch = /^\/api\/v1\/groups\/([^/]+)(?:\/(.*))?$/.exec(url.pathname);
  if (groupMatch && actorsByGroup[groupMatch[1]]) {
    const [, groupId, resource] = groupMatch;
    if (resource === "actors") {
      const held = nextActorRead?.groupId === groupId ? nextActorRead : null;
      if (held) nextActorRead = null;
      const actors = held ? await held.result : structuredClone(actorsByGroup[groupId]);
      // Bound the unchanged-response loop so both sides use identical latency.
      await new Promise((resolve) => window.setTimeout(resolve, 25));
      return jsonResponse({ actors });
    }
    if (!resource) return jsonResponse({ group: groupFixture(groupId) });
    if (resource === "ledger/tail") return jsonResponse({ events: [], has_more: false });
    if (resource === "context") return jsonResponse({ agent_states: [] });
    if (resource === "settings") return jsonResponse({ settings: {} });
    if (resource === "presentation") return jsonResponse({ presentation: null });
  }
  const recoveryMatch = /^\/api\/v1\/local_sessions\/recovery\/([^/]+)\/([^/]+)$/.exec(
    url.pathname,
  );
  if (recoveryMatch) {
    const [, groupId, actorId] = recoveryMatch;
    const actor = actorsByGroup[groupId].find((item) => item.id === actorId)!;
    return jsonResponse({
      group_id: groupId,
      actor_id: actorId,
      source_id: actor.external_session_id,
      recovery: {
        state: actor.running ? "connected" : "disconnected",
        cwd: `/synthetic/${groupId}`,
        shell: "posix",
        command: `server-command-${groupId}`,
      },
    });
  }
  unexpectedRequests.push(`${method} ${url.pathname}`);
  throw new Error(`Unexpected read: ${url.pathname}`);
}

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await i18nReady;
  await i18n.changeLanguage("en");
  // Settle lazy module loading before advancing the synthetic clock.
  await import("./components/AgentTab");
  localStorage.clear();
  localStorage.setItem("cccc-theme", "dark");
  clearAuthToken();
  vi.useFakeTimers();
  requests = [];
  unexpectedRequests = [];
  nextActorRead = null;
  releasePendingReads = [];
  actorsByGroup = {
    [GROUP_A]: [actorFixture("native")],
    [GROUP_B]: [actorFixture("native", "Beta worker")],
  };
  groupViewCache.clear();
  useGroupStore.setState(useGroupStore.getInitialState(), true);
  useUIStore.setState({ ...useUIStore.getInitialState(), canAccessGlobalSettings: true }, true);
  useModalStore.setState(useModalStore.getInitialState(), true);
  useComposerStore.setState(useComposerStore.getInitialState(), true);
  useObservabilityStore.setState({ loaded: true });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  vi.stubGlobal("fetch", vi.fn(serveRequest));
  vi.stubGlobal(
    "WebSocket",
    class {
      constructor() {
        unexpectedRequests.push("WebSocket");
        throw new Error("Unexpected WebSocket in the isolated HTTP fixture");
      }
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  await act(async () => releasePendingReads.forEach((release) => release()));
  await advance(100);
  host.remove();
  expect(refreshActorsInFlight.size).toBe(0);
  expect(refreshActorsQueued.size).toBe(0);
  expect(loadGroupInFlight.size).toBe(0);
  expect(deferredUnreadRefreshTimers.size).toBe(0);
  expect(unexpectedRequests).toEqual([]);
  // The page-owned Group cache intentionally outlives App; retire the fixture
  // and its five-minute expiry before checking for leaked reconciliation work.
  groupViewCache.clear();
  expect(vi.getTimerCount()).toBe(0);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function advance(milliseconds: number) {
  // Flush React after each simulated network round trip, rather than batching
  // ten seconds into one render (which would accidentally conceal the loop).
  for (let remaining = milliseconds; remaining > 0; remaining -= Math.min(50, remaining)) {
    await act(async () => vi.advanceTimersByTimeAsync(Math.min(50, remaining)));
  }
}

async function click(label: string) {
  const button = [...host.querySelectorAll("button")].find((node) => node.textContent === label);
  expect(button, `Missing visible control ${label}`).toBeDefined();
  await act(async () => button!.click());
}

async function openApp(kind: ActorKind, actorPatch: Partial<Actor> = {}) {
  actorsByGroup[GROUP_A] = [{ ...actorFixture(kind), ...actorPatch }];
  useGroupStore.getState().setGroups([groupFixture(GROUP_A), groupFixture(GROUP_B)]);
  useGroupStore.getState().setSelectedGroupId(GROUP_A);
  await act(async () => root.render(<App />));
  await advance(500);
  expect(useGroupStore.getState().selectedGroupActorsHydrating).toBe(false);
  expect(actorRequests(GROUP_A).length).toBeGreaterThanOrEqual(2);
  requests = [];
}

function actorRequests(groupId: string) {
  return requests.filter((request) => request.pathname === `/api/v1/groups/${groupId}/actors`);
}

function visibleActor() {
  const pane = host.querySelector<HTMLElement>("article:not([hidden])");
  expect(pane).not.toBeNull();
  return pane!;
}

it.each(["native", "managed"] as const)(
  "bounds visible enabled stopped %s reconciliation to 4 Actor HTTP GETs in 10 seconds despite unchanged responses and an App rerender",
  async (kind) => {
    await openApp(kind);
    await click("Open Alpha worker");
    await advance(1000);
    await click("Light theme");
    await advance(9000);
    expect(visibleActor().textContent).toContain(
      kind === "native" ? enActors.nativeSession.unavailable : enActors.stopped,
    );
    expect(actorRequests(GROUP_A)).toHaveLength(4);
    const reads = actorRequests(GROUP_A);
    expect(reads.map((request) => request.at - reads[0].at)).toEqual([0, 3000, 6000, 9000]);
    expect(actorRequests(GROUP_B)).toHaveLength(0);
    expect(requests.every((request) => request.method === "GET")).toBe(true);
    expect(
      requests.filter((request) => request.pathname.includes("/local_sessions/recovery/")),
    ).toHaveLength(kind === "native" ? 1 : 0);
  },
);

it.each([
  { kind: "native", state: "running", patch: { running: true } },
  { kind: "managed", state: "running", patch: { running: true } },
  { kind: "native", state: "disabled", patch: { enabled: false } },
  { kind: "managed", state: "disabled", patch: { enabled: false } },
] as const)("does not reconcile a $state $kind Actor", async ({ kind, patch }) => {
  await openApp(kind, patch);
  await click("Open Alpha worker");
  await advance(10000);
  expect(visibleActor().textContent).toContain(
    kind === "native"
      ? patch.running
        ? enActors.nativeSession.available
        : enActors.webAccessPaused
      : patch.running
        ? enActors.running
        : enActors.stopped,
  );
  expect(actorRequests(GROUP_A)).toHaveLength(0);
});

it.each(["native", "managed"] as const)(
  "stops reconciling when a %s pane is hidden",
  async (kind) => {
    await openApp(kind);
    await click("Open Alpha worker");
    await advance(100);
    await click("Messages");
    expect(host.querySelector("article:not([hidden])")).toBeNull();
    requests = [];
    await advance(10000);
    expect(actorRequests(GROUP_A)).toHaveLength(0);
    expect(
      requests.filter((request) => request.pathname.includes("/local_sessions/")),
    ).toHaveLength(0);
  },
);

it.each(["native", "managed"] as const)(
  "reconciles an authoritative %s running transition and stops further status reads",
  async (kind) => {
    await openApp(kind);
    await click("Open Alpha worker");
    await advance(1000);
    actorsByGroup[GROUP_A][0] = { ...actorsByGroup[GROUP_A][0], running: true };
    await advance(2500);
    expect(visibleActor().textContent).toContain(
      kind === "native" ? enActors.nativeSession.available : enActors.running,
    );
    expect(actorRequests(GROUP_A)).toHaveLength(2);
    requests = [];
    await advance(6000);
    expect(actorRequests(GROUP_A)).toHaveLength(0);
  },
);

it("refreshes the newly selected Group, not the old closure, and ignores its late status response", async () => {
  await openApp("native");
  await click("Open Alpha worker");
  await advance(100);
  let release!: (actors: Actor[]) => void;
  nextActorRead = {
    groupId: GROUP_A,
    result: new Promise((resolve) => {
      release = resolve;
    }),
  };
  releasePendingReads.push(() => release(actorsByGroup[GROUP_A]));
  await advance(3000);
  expect(nextActorRead).toBeNull();
  await click("Beta Group");
  await advance(500);
  expect(useGroupStore.getState().selectedGroupId).toBe(GROUP_B);
  requests = [];
  await click("Open Beta worker");
  await advance(100);
  await act(async () => release([{ ...actorFixture("native"), running: true }]));
  await advance(9900);
  expect(visibleActor().getAttribute("aria-label")).toBe(`${GROUP_B}/worker`);
  expect(visibleActor().textContent).toContain("Beta worker");
  expect(visibleActor().textContent).toContain(enActors.nativeSession.unavailable);
  expect(visibleActor().textContent).not.toContain("Alpha worker");
  expect(actorRequests(GROUP_A)).toHaveLength(0);
  expect(actorRequests(GROUP_B)).toHaveLength(4);
});
