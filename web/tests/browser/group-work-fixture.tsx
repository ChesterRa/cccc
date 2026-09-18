// Full AppShell / chat / terminal components. Every transport is synthetic and local to this page.
import { createRef, useEffect, useState, type ComponentProps } from "react";
import { Terminal } from "@xterm/xterm";
import i18next from "../../src/i18n";
import { MobileMenuSheet } from "../../src/components/layout/MobileMenuSheet";
import { AppShell } from "../../src/components/app/AppShell";
import { SearchModal } from "../../src/components/SearchModal";
import { PresentationViewerModal } from "../../src/components/presentation/PresentationViewerModal";
import { SettingsModal } from "../../src/components/SettingsModal";
import { ImagePreview } from "../../src/components/messageBubble/ImagePreview";
import { MarkdownRenderer } from "../../src/components/MarkdownRenderer";
import { PresentationPinModal } from "../../src/components/presentation/PresentationPinModal";
import { useGroupActions } from "../../src/hooks/useGroupActions";
import { useTextScale } from "../../src/hooks/useTextScale";
import { useTheme } from "../../src/hooks/useTheme";
import {
  useGroupStore,
  useUIStore,
  useComposerStore,
  useObservabilityStore,
  useModalStore,
} from "../../src/stores";
import type { Actor, GroupDoc, GroupMeta, LedgerEvent } from "../../src/types";
import "../../src/index.css";

const probe = {
  terminals: [] as Terminal[],
  sockets: [] as FixtureSocket[],
  requests: [] as { path: string; method: string; body: unknown }[],
  errors: [] as string[],
  actions: [] as string[],
  externalWriters: new Set<string>(),
  catalogDelay: 0,
  catalogRestricted: false,
  searchMode: "results",
  searchDelay: 0,
  brandingName: "CCCC",
  globalAllowed: true,
  runDelay: 0,
  runFailure: false,
  notebookWarning: "",
};
const openTerminal = Terminal.prototype.open;
Terminal.prototype.open = function (parent) {
  openTerminal.call(this, parent);
  probe.terminals.push(this);
};
window.addEventListener("error", (event) => probe.errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => probe.errors.push(String(event.reason)));
const actors: Actor[] = Array.from({ length: 8 }, (_, i) => ({
  id: `actor-${i + 1}`,
  title: [
    "Foreman",
    "Implementation",
    "Tests",
    "Review",
    "Documentation",
    "Integration",
    "Release",
    "Research",
  ][i],
  role: i ? "peer" : "foreman",
  runtime: "codex",
  runner: "pty",
  running: true,
  enabled: true,
  effective_working_state: "working",
  runtime_state_source: "managed_session",
}));
const doc = (groupId: string): GroupDoc =>
  ({
    group_id: groupId,
    title: groupId === "g1" ? "Release workspace" : "Research workspace",
    state: "active",
    active_scope_key: "fixture",
    scopes: [{ scope_key: "fixture", url: "/synthetic/project" }],
    actors,
  }) as GroupDoc;
function withRuntime(group: GroupMeta): GroupMeta {
  return {
    ...group,
    runtime_status: {
      lifecycle_state: group.state || "active",
      runtime_running: !!group.running,
      running_actor_count: group.running ? actors.length : 0,
      has_running_foreman: !!group.running,
    },
  };
}
let groups = ["g1", "g2"].map(
  (id) => ({ group_id: id, title: doc(id).title, running: true, state: "active" }) as GroupMeta,
);
function seed(groupId: string) {
  useGroupStore.setState({
    selectedGroupId: groupId,
    groupDoc: {
      ...doc(groupId),
      ...withRuntime(groups.find((group) => group.group_id === groupId)!),
    },
    actors,
    groups,
    groupContext: { agent_states: [] },
    groupPresentation: {
      v: 1,
      updated_at: "2026-09-06T00:00:00Z",
      slots: [
        {
          slot_id: "slot-1",
          card: {
            title: "Release checklist",
            card_type: "markdown",
            content: { mode: "inline", markdown: "# Release\n\nAll checks have finished." },
            updated_at: "2026-09-06T00:00:00Z",
          },
        },
      ],
    },
  });
  if (!useGroupStore.getState().chatByGroup[groupId]?.events.length) {
    for (let i = 0; i < 30; i++)
      useGroupStore
        .getState()
        .appendEvent(
          {
            id: `${groupId}-event-${i}`,
            ts: `2026-09-06T00:00:${String(i).padStart(2, "0")}Z`,
            group_id: groupId,
            kind: "chat.message",
            by: i % 2 ? "actor-1" : "user",
            data: {
              text: `Message ${i}: ${i % 2 ? "The regression suite passed. Continuing the next verification." : "Please continue checking the work and send the result here."}`,
              to: [i % 2 ? "user" : "actor-1"],
              message_mode: "send",
            },
          } as LedgerEvent,
          groupId,
        );
  }
}
seed("g1");
useObservabilityStore.setState({ loaded: true });
useComposerStore.getState().switchGroup(null, "g1");
window.fetch = async (input, init) => {
  const url = new URL(String(input), location.href);
  if (
    url.pathname === "/ui/reading-example.svg" ||
    url.pathname.endsWith("/blobs/reading-example.svg")
  ) {
    return new Response(
      '<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900"><rect width="1600" height="900" fill="#edf2f7"/><path d="M200 450H1400" stroke="#304050" stroke-width="8"/><text x="200" y="420" font-size="60" fill="#304050">Architecture drawing</text></svg>',
      { headers: { "Content-Type": "image/svg+xml" } },
    );
  }
  const body = init?.body && typeof init.body === "string" ? JSON.parse(init.body) : {};
  probe.requests.push({ path: url.pathname, method: init?.method || "GET", body });
  if (url.pathname === "/api/v1/ping")
    return Response.json({
      ok: true,
      result: {
        version: "0.4.40",
        build: { source_id: "a".repeat(64) },
        daemon: { version: "0.4.40", build: { source_id: "a".repeat(64) } },
        web: { assets_id: "b".repeat(64), entry_script: "/ui/assets/fixture-entry.js" },
      },
    });
  let result: unknown = {};
  const runRoute = url.pathname.match(/^\/api\/v1\/groups\/([^/]+)\/(start|stop|state)$/);
  if (runRoute && init?.method === "POST") {
    if (probe.runDelay) await new Promise((resolve) => setTimeout(resolve, probe.runDelay));
    if (probe.runFailure)
      return Response.json(
        { ok: false, error: { code: "fixture", message: "Runtime rejected the operation" } },
        { status: 409 },
      );
    const [, id, action] = runRoute;
    groups = groups.map((group) =>
      group.group_id !== id
        ? group
        : {
            ...group,
            state:
              action === "state"
                ? (url.searchParams.get("state") as GroupMeta["state"])
                : action === "start"
                  ? "active"
                  : "stopped",
            running: action === "state" ? group.running : action === "start",
          },
    );
    groups = groups.map(withRuntime);
    return Response.json({
      ok: true,
      result: { group: { ...doc(id), ...groups.find((group) => group.group_id === id) } },
    });
  }
  if (url.pathname === "/api/v1/groups") return Response.json({ ok: true, result: { groups } });
  if (url.pathname.endsWith("/ledger/search")) {
    const mode = probe.searchMode;
    const query = url.searchParams.get("q") || "";
    if (probe.searchDelay) await new Promise((resolve) => setTimeout(resolve, probe.searchDelay));
    if (mode === "error")
      return Response.json(
        { ok: false, error: { code: "fixture", message: "Search temporarily unavailable" } },
        { status: 503 },
      );
    return Response.json({
      ok: true,
      result: {
        events:
          mode === "empty"
            ? []
            : [
                {
                  id: "fixture-search-1",
                  group_id: "g1",
                  by: "actor-1",
                  ts: "2026-09-17T02:32:00Z",
                  kind: "chat.message",
                  data: {
                    text: `${query || "Release"}: checks completed. The next step is to review the deployment checklist together.`,
                  },
                },
              ],
        has_more: false,
      },
    });
  }
  if (url.pathname === "/api/v1/branding") {
    if (body.product_name) probe.brandingName = body.product_name;
    return Response.json({
      ok: true,
      result: {
        branding: {
          product_name: probe.brandingName,
          logo_icon_url: "/ui/logo.svg",
          favicon_url: "/ui/logo.svg",
          has_custom_logo_icon: false,
          has_custom_favicon: false,
        },
      },
    });
  }
  if (url.pathname === "/api/v1/web_access/session")
    return Response.json({
      ok: true,
      result: {
        web_access_session: { login_active: true, can_access_global_settings: probe.globalAllowed },
      },
    });
  if (url.pathname === "/api/v1/membership")
    return Response.json({
      ok: true,
      result: {
        membership: {
          enabled: false,
          logged_in: false,
          account_reachable: true,
          reach_supported: true,
        },
      },
    });
  if (url.pathname === "/api/v1/profiles")
    return Response.json({
      ok: true,
      result: {
        profiles: [
          {
            id: "profile-fixture",
            name: "Review and implementation",
            runtime: "codex",
            revision: 2,
            usage_count: 3,
          },
        ],
      },
    });
  if (url.pathname.endsWith("/space/status"))
    return Response.json({
      ok: true,
      result: {
        provider: { auth_configured: true, write_ready: true, last_error: probe.notebookWarning },
        bindings: {
          work: { remote_space_id: "notebook-fixture", status: "bound" },
          memory: { remote_space_id: "notebook-fixture", status: "bound" },
        },
      },
    });
  if (url.pathname.endsWith("/space/spaces"))
    return Response.json({
      ok: true,
      result: {
        spaces: [
          { remote_space_id: "notebook-fixture", title: "CCCC · Release workspace" },
          { remote_space_id: "notebook-next", title: "Next project" },
        ],
      },
    });
  if (url.pathname.endsWith("/notebooklm/auth"))
    return Response.json({
      ok: true,
      result: { auth: { state: "idle", message: "Saved Google session is verified." } },
    });
  if (url.pathname.endsWith("/workspace/list"))
    return Response.json({
      ok: true,
      result: {
        scope_key: "fixture",
        scope_url: "/synthetic/project",
        root_path: "/synthetic/project",
        path: url.searchParams.get("path") || "",
        parent: null,
        items: [
          { name: "src", path: "src", is_dir: true },
          { name: "README.md", path: "README.md", is_dir: false },
          { name: "package.json", path: "package.json", is_dir: false },
        ],
      },
    });
  if (url.pathname.endsWith("/connect/catalog")) {
    if (probe.catalogDelay) await new Promise((resolve) => setTimeout(resolve, probe.catalogDelay));
    if (probe.catalogRestricted)
      return Response.json(
        { ok: false, error: { code: "admin_required", message: "administrator access required" } },
        { status: 403 },
      );
    const instance = url.searchParams.get("instance_id");
    result = instance
      ? {
          instance: {
            instance_id: instance,
            display_name: instance === "i_mac" ? "Mac Studio" : "Direct workstation",
          },
          catalog: {
            groups: [
              {
                group_id: "g1",
                title: "Shared Team",
                actors: [{ id: "remote-worker", title: "Remote worker", enabled: true }],
              },
            ],
          },
          fresh: instance === "i_mac",
          next: null,
        }
      : {
          instances: [{ instance_id: "i_mac", display_name: "Mac Studio" }],
          external_groups: [
            {
              instance: { instance_id: "i_direct", display_name: "Direct workstation" },
              group_id: "g1",
              title: "Shared Team",
              transport: "direct",
            },
          ],
        };
    return Response.json({ ok: true, result });
  }
  if (url.pathname.endsWith("/codex_voice/calls/active"))
    result = { call: null, analyst: null, readiness: null, voices: [] };
  else if (url.pathname.endsWith("/codex_voice/messages/viewed"))
    result = { observed: body.messages.length };
  else if (url.pathname.endsWith("/terminal/tail"))
    result = { text: "◦ Working (esc to interrupt)\n", running: true };
  else if (url.pathname.endsWith("/actors"))
    result = {
      actors: actors.map((actor) => ({
        ...actor,
        running:
          groups.find((group) => group.group_id === url.pathname.split("/")[4])?.running ?? true,
      })),
    };
  else if (url.pathname.endsWith("/capabilities")) result = { capabilities: [] };
  else if (url.pathname.includes("/context")) result = { agent_states: [] };
  else if (url.pathname.includes("/presentation"))
    result = { presentation: useGroupStore.getState().groupPresentation };
  else if (url.pathname.endsWith("/send"))
    result = {
      event: {
        id: "sent-fixture",
        ts: new Date().toISOString(),
        kind: "chat.message",
        by: "user",
        data: { text: body.text, to: body.to, message_mode: "send" },
      },
    };
  else if (url.pathname.includes("messages/window"))
    result = {
      events:
        useGroupStore.getState().chatByGroup[useGroupStore.getState().selectedGroupId]?.events ||
        [],
      center_index: 10,
    };
  else if (url.pathname.includes("/skills")) result = { skills: [] };
  else if (url.pathname.includes("/settings")) result = { settings: {} };
  else if (url.pathname.includes("/messages")) result = { messages: [] };
  return Response.json({ ok: true, result });
};
class FixtureSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSED = 3;
  readyState = 0;
  binaryType = "arraybuffer";
  onopen?: (event: unknown) => void;
  onmessage?: (event: { data: ArrayBuffer | string }) => void;
  onclose?: (event: { code: number }) => void;
  onerror?: (event: unknown) => void;
  frames: { type: number; text: string }[] = [];
  actor: string;
  group: string;
  private timer: ReturnType<typeof setInterval> | undefined;
  private realtime: boolean;
  constructor(public url: string) {
    const parsed = new URL(url);
    this.realtime = parsed.pathname === "/api/v1/events/ws";
    this.actor = parsed.pathname.split("/").at(-2)!;
    this.group = parsed.pathname.split("/")[4];
    const since = Number(parsed.searchParams.get("since") || 0);
    if (!this.realtime) probe.sockets.push(this);
    setTimeout(() => {
      if (this.readyState === 3) return;
      this.readyState = 1;
      this.onopen?.({});
      if (this.realtime) return;
      if (parsed.searchParams.get("takeover") === "true") probe.externalWriters.delete(this.actor);
      this.onmessage?.({
        data: JSON.stringify({
          type: "terminal.attach",
          ok: true,
          result: {
            terminal_writable:
              parsed.searchParams.get("mode") !== "viewer" &&
              !probe.externalWriters.has(this.actor),
            replay_cursor: since,
            replay_end_cursor: since,
          },
        }),
      });
      if (!since)
        this.output(
          `\x1b[36m${this.actor}\x1b[0m — ${this.group}\r\n\r\nReviewing the implementation and running focused tests.\r\n\r\n$ `,
        );
      let tick = 0;
      this.timer = setInterval(
        () => this.output(`\r\x1b[K◦ Working: checked ${++tick} files (esc to interrupt)`),
        1500,
      );
    }, 10);
  }
  output(text: string) {
    if (this.readyState !== 1) return;
    const payload = new TextEncoder().encode(text);
    const frame = new Uint8Array(payload.length + 1);
    frame[0] = 49;
    frame.set(payload, 1);
    this.onmessage?.({ data: frame.buffer });
  }
  send(data: ArrayBuffer | Uint8Array | string) {
    if (typeof data === "string") {
      const packet = JSON.parse(data);
      if (this.realtime && packet.type === "subscribe")
        queueMicrotask(() => {
          if (this.readyState !== 1) return;
          this.onmessage?.({
            data: JSON.stringify({ type: "ready", channel: packet.channel, id: packet.id }),
          });
          if (packet.channel === "headless")
            this.onmessage?.({
              data: JSON.stringify({
                type: "event",
                channel: packet.channel,
                id: packet.id,
                message: { event: "headless.snapshot", data: { events: [] } },
              }),
            });
        });
      return;
    }
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    const text = new TextDecoder().decode(bytes.slice(1));
    this.frames.push({ type: bytes[0], text });
    if (bytes[0] === 48) this.output(text.replace(/\r/g, "\r\n"));
  }
  close() {
    this.readyState = 3;
    clearInterval(this.timer);
  }
}
window.WebSocket = FixtureSocket as unknown as typeof WebSocket;
const noop = () => {};
const composerRef = createRef<HTMLTextAreaElement>();
const fileInputRef = createRef<HTMLInputElement>();
const eventContainerRef = { current: null as HTMLDivElement | null };
const contentRef = { current: null as HTMLDivElement | null };
const chatAtBottomRef = { current: true };
export function Fixture() {
  const { handleGroupControl, handleDeleteGroup } = useGroupActions();
  const currentGroups = useGroupStore((state) => state.groups);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const groupId = useGroupStore((state) => state.selectedGroupId);
  const presentation = useGroupStore((state) => state.groupPresentation);
  const presentationViewer = useModalStore((state) => state.presentationViewer);
  const currentActors = useGroupStore((state) => state.actors);
  const currentDoc = useGroupStore((state) => state.groupDoc);
  const selectedGroupRunning = useGroupStore(
    (state) => state.groups.find((group) => group.group_id === groupId)?.running ?? false,
  );
  const activeTab = useUIStore((state) => state.activeTab);
  const [width, setWidth] = useState(innerWidth);
  const { theme, setTheme, isDark: dark } = useTheme();
  const { textScale, setTextScale } = useTextScale();
  const [readOnly, setReadOnly] = useState(false);
  const [canAccessAccount, setCanAccessAccount] = useState(true);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [readingExamples, setReadingExamples] = useState(false);
  const [connectionStatus, setConnectionStatus] = useState<
    "connected" | "connecting" | "disconnected"
  >("connected");
  const presentationPin = useModalStore((state) => state.presentationPin);
  const [showMentionMenu, setShowMentionMenu] = useState(false);
  const [mentionFilter, setMentionFilter] = useState("");
  const [mentionKind, setMentionKind] = useState<"agent" | "group">("agent");
  const [mentionActorScope, setMentionActorScope] = useState<"selected" | "destination">(
    "selected",
  );
  const [mentionSelectedIndex, setMentionSelectedIndex] = useState(0);
  useEffect(() => {
    const update = () => {
      setWidth(innerWidth);
      useUIStore.getState().setSmallScreen(innerWidth < 768);
    };
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);
  const changeGroup = (id: string) => {
    useComposerStore.getState().switchGroup(groupId, id);
    seed(id);
    useUIStore.getState().setActiveTab("chat");
    useComposerStore.getState().setDestGroupId(id);
  };
  Object.assign(window, {
    groupWorkProbe: {
      ...probe,
      chooseGroup: changeGroup,
      openSearch: () => setSearchOpen(true),
      openSettings: (scope: "group" | "global", tab: string) => {
        useModalStore.getState().openSettingsTarget({ scope, tab });
        setSettingsOpen(true);
      },
      setSearchMode: (mode: string, delay = 0) => {
        probe.searchMode = mode;
        probe.searchDelay = delay;
      },
      setNotebookWarning: (warning: string) => {
        probe.notebookWarning = warning;
      },
      setGlobalAllowed: (value: boolean) => {
        probe.globalAllowed = value;
      },
      setCatalogDelay: (ms: number) => {
        probe.catalogDelay = ms;
      },
      setCatalogRestricted: (value: boolean) => {
        probe.catalogRestricted = value;
      },
      setRunning: (running: boolean) => {
        groups = groups.map((group) =>
          group.group_id === groupId ? withRuntime({ ...group, running }) : group,
        );
        useGroupStore.setState((state) => ({
          groups,
          groupDoc: { ...state.groupDoc!, ...groups.find((group) => group.group_id === groupId) },
          actors: state.actors.map((actor) => ({ ...actor, running })),
        }));
      },
      setCount: (count: number) => useGroupStore.setState({ actors: actors.slice(0, count) }),
      patchActor: (id: string, patch: Partial<Actor>) =>
        useGroupStore.setState((state) => ({
          actors: state.actors.map((actor) => (actor.id === id ? { ...actor, ...patch } : actor)),
        })),
      setRunTransport: (delay = 0, failure = false) => {
        probe.runDelay = delay;
        probe.runFailure = failure;
      },
      setGroupStatus: (id: string, state: GroupMeta["state"], running: boolean) => {
        groups = groups.map((group) =>
          group.group_id === id ? withRuntime({ ...group, state, running }) : group,
        );
        useGroupStore.setState((current) => ({
          groups,
          groupDoc:
            current.groupDoc?.group_id === id
              ? { ...current.groupDoc, ...groups.find((group) => group.group_id === id) }
              : current.groupDoc,
        }));
      },
      setSidebarCollapsed,
      setReadingExamples,
      setConnectionStatus,
      setReadOnly,
      setCanAccessAccount,
      setTextScale,
      setDark: (value: boolean) => {
        setTheme(value ? "dark" : "light");
      },
      language: (lang: string) => i18next.changeLanguage(lang),
      ui: useUIStore,
      group: useGroupStore,
      composer: useComposerStore,
      modals: useModalStore,
    },
  });
  const props = {
    canUseVoice: true,
    canAccessAccount,
    orderedGroups: currentGroups,
    archivedGroupIds: [],
    selectedGroupId: groupId,
    groupDoc: currentDoc,
    groupContext: { agent_states: [] },
    actors: currentActors,
    runtimeActors: currentActors,
    recipientActors: currentActors,
    recipientActorsBusy: false,
    destGroupScopeLabel: "",
    activeTab,
    busy: "",
    isTransitioning: false,
    sidebarOpen,
    sidebarCollapsed,
    sidebarWidth: 248,
    isDark: dark,
    isSmallScreen: width < 768,
    webReadOnly: readOnly,
    selectedGroupRunning,
    selectedGroupRuntimeStatus: null,
    selectedGroupActorsHydrating: false,
    selectedGroupActorStatusProvisional: false,
    theme,
    textScale,
    sseStatus: connectionStatus,
    groupLabelById: { g1: "Release workspace", g2: "Research workspace" },
    mentionSelectedIndex,
    showMentionMenu,
    mentionFilter,
    mentionKind,
    mentionActorScope,
    setMentionSelectedIndex,
    setShowMentionMenu,
    setMentionFilter,
    setMentionKind,
    setMentionActorScope,
    composerRef,
    fileInputRef,
    eventContainerRef,
    contentRef,
    chatAtBottomRef,
    onSelectGroup: changeGroup,
    onTabChange: (tab: string) => {
      useUIStore.getState().setActiveTab(tab);
    },
    getTermEpoch: () => 0,
  } as ComponentProps<typeof AppShell>;
  for (const name of [
    "onThemeChange",
    "onTextScaleChange",
    "onWarmGroup",
    "onCloseSidebar",
    "onToggleSidebar",
    "onResizeSidebar",
    "onReorderGroupsInSection",
    "onArchiveGroup",
    "onRestoreGroup",
    "onOpenSidebar",
    "onOpenSearch",
    "onOpenContext",
    "onStartGroup",
    "onOpenSettings",
    "onOpenAccount",
    "onOpenMobileMenu",
    "appendComposerFiles",
    "setMentionTargetGroupId",
    "onToggleActorEnabled",
    "onRelaunchActor",
    "onNewActorSession",
    "onEditActor",
    "onRemoveActor",
    "onOpenActorInbox",
    "onRefreshActors",
    "onTouchStart",
    "onTouchEnd",
  ])
    Object.assign(props, { [name]: noop });
  props.onOpenSidebar = () => setSidebarOpen(true);
  props.onCloseSidebar = () => setSidebarOpen(false);
  props.onOpenMobileMenu = () => setMenuOpen(true);
  props.onThemeChange = setTheme;
  props.onTextScaleChange = setTextScale;
  props.onOpenSettings = () => setSettingsOpen(true);
  props.onOpenAccount = () => {
    probe.actions.push("onOpenAccount");
    setSettingsOpen(true);
  };
  props.onOpenGroupEdit = () => probe.actions.push("onOpenGroupEdit");
  props.onOpenContext = () => probe.actions.push("onOpenContext");
  props.onOpenSearch = () => probe.actions.push("onOpenSearch");
  props.onStartGroup = () => probe.actions.push("onStartGroup");
  props.onControlGroup = handleGroupControl;
  props.onDeleteGroup = handleDeleteGroup;
  for (const name of [
    "onToggleActorEnabled",
    "onRelaunchActor",
    "onNewActorSession",
    "onEditActor",
    "onRemoveActor",
    "onOpenActorInbox",
  ] as const)
    Object.assign(props, { [name]: (actor: Actor) => probe.actions.push(`${name}:${actor.id}`) });
  return (
    <div className="h-dvh">
      <AppShell {...props} />
      {readingExamples && (
        <section
          data-reading-examples
          className="fixed inset-x-6 top-20 z-40 max-h-[65dvh] overflow-auto rounded-xl border bg-[var(--color-bg-primary)] p-4"
        >
          <ImagePreview
            href="/ui/reading-example.svg"
            downloadHref="/ui/reading-example.svg"
            downloadName="drawing.svg"
            alt="Architecture drawing"
            isSvg
            isUserMessage={false}
            isDark={dark}
          />
          <MarkdownRenderer
            enableMermaid
            isDark={dark}
            content={"```mermaid\nflowchart LR\nA[Prepare] --> B[Review]\n```"}
          />
        </section>
      )}
      {presentationPin && (
        <PresentationPinModal
          isOpen
          isDark={dark}
          groupId={groupId}
          slot={{
            slot_id: presentationPin.slotId,
            index: Number(presentationPin.slotId.slice(-1)),
          }}
          busy={false}
          onClose={() => useModalStore.getState().setPresentationPin(null)}
          onSubmitUrl={() => probe.actions.push("pinUrl")}
          onSubmitFile={() => probe.actions.push("pinFile")}
          onSubmitWorkspace={() => probe.actions.push("pinWorkspace")}
        />
      )}
      <MobileMenuSheet
        isOpen={menuOpen}
        onClose={() => setMenuOpen(false)}
        theme={theme}
        textScale={textScale}
        selectedGroupId={groupId}
        groupDoc={currentDoc}
        selectedGroupRunning={selectedGroupRunning}
        onThemeChange={setTheme}
        onTextScaleChange={setTextScale}
        onOpenSearch={noop}
        onOpenContext={noop}
        onOpenSettings={props.onOpenSettings}
        canAccessAccount={canAccessAccount}
        onOpenAccount={props.onOpenAccount}
      />
      <SearchModal
        isOpen={searchOpen}
        onClose={() => setSearchOpen(false)}
        groupId={groupId}
        groupTitle={currentDoc?.title}
        actors={currentActors}
        isDark={dark}
        onReply={() => {
          probe.actions.push("searchReply");
          setSearchOpen(false);
        }}
        onJumpToMessage={() => {
          probe.actions.push("searchContext");
          setSearchOpen(false);
        }}
      />
      {presentationViewer && presentationViewer.surface !== "split" && (
        <PresentationViewerModal
          key={`${presentationViewer.groupId}:${presentationViewer.slotId}`}
          isOpen
          isDark={dark}
          readOnly={readOnly}
          groupId={presentationViewer.groupId}
          slotId={presentationViewer.slotId}
          presentation={presentation}
          focusRef={presentationViewer.focusRef}
          focusEventId={presentationViewer.focusEventId}
          onQuoteInChat={() => probe.actions.push("quote")}
          supportsSplit={width >= 768}
          onOpenSplit={() => {
            useUIStore.getState().setChatPresentationDisplayMode(groupId, "split");
            useModalStore
              .getState()
              .setPresentationViewer({ ...presentationViewer, surface: "split" });
          }}
          onSelectSlot={(slotId) =>
            useModalStore.getState().setPresentationViewer({ groupId, slotId, surface: "modal" })
          }
          onPinSlot={(slotId) => {
            useModalStore.getState().setPresentationViewer(null);
            useModalStore.getState().setPresentationPin({ groupId, slotId });
          }}
          onClose={() => useModalStore.getState().setPresentationViewer(null)}
        />
      )}
      {settingsOpen ? (
        <SettingsModal
          isOpen
          onClose={() => setSettingsOpen(false)}
          settings={null}
          onUpdateSettings={async () => true}
          busy={false}
          isDark={dark}
          groupId={groupId}
          groupDoc={doc(groupId)}
        />
      ) : null}
    </div>
  );
}
