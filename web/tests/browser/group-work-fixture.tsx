// Full AppShell / chat / terminal components. Every transport is synthetic and local to this page.
import { createRef, useEffect, useState, type ComponentProps } from "react";
import { Terminal } from "@xterm/xterm";
import i18next from "../../src/i18n";
import { MobileMenuSheet } from "../../src/components/layout/MobileMenuSheet";
import { AppShell } from "../../src/components/app/AppShell";
import { SettingsModal } from "../../src/components/SettingsModal";
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
const groups = ["g1", "g2"].map(
  (id) => ({ group_id: id, title: doc(id).title, running: true, state: "active" }) as GroupMeta,
);
function seed(groupId: string) {
  useGroupStore.setState({
    selectedGroupId: groupId,
    groupDoc: doc(groupId),
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
            content: { markdown: "# Release\n\nAll checks have finished." },
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
useComposerStore.getState().setDestGroupId("g1");
window.fetch = async (input, init) => {
  const url = new URL(String(input), location.href);
  const body = init?.body && typeof init.body === "string" ? JSON.parse(init.body) : {};
  probe.requests.push({ path: url.pathname, method: init?.method || "GET", body });
  let result: unknown = {};
  if (url.pathname.endsWith("/codex_voice/calls/active"))
    result = { call: null, analyst: null, readiness: null, voices: [] };
  else if (url.pathname.endsWith("/codex_voice/messages/viewed"))
    result = { observed: body.messages.length };
  else if (url.pathname.endsWith("/terminal/tail"))
    result = { text: "◦ Working (esc to interrupt)\n", running: true };
  else if (url.pathname.endsWith("/actors")) result = { actors };
  else if (url.pathname.endsWith("/capabilities")) result = { capabilities: [] };
  else if (url.pathname.includes("/context")) result = { agent_states: [] };
  else if (url.pathname.includes("/presentation"))
    result = { presentation: useGroupStore.getState().groupPresentation };
  else if (url.pathname.includes("messages/send"))
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
  constructor(public url: string) {
    const parsed = new URL(url);
    this.actor = parsed.pathname.split("/").at(-2)!;
    this.group = parsed.pathname.split("/")[4];
    probe.sockets.push(this);
    setTimeout(() => {
      if (this.readyState === 3) return;
      this.readyState = 1;
      this.onopen?.({});
      if (parsed.searchParams.get("takeover") === "true") probe.externalWriters.delete(this.actor);
      this.onmessage?.({
        data: JSON.stringify({
          type: "terminal.attach",
          ok: true,
          result: {
            terminal_writable:
              parsed.searchParams.get("mode") !== "viewer" &&
              !probe.externalWriters.has(this.actor),
            replay_cursor: 0,
            replay_end_cursor: 0,
          },
        }),
      });
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
  send(data: ArrayBuffer | Uint8Array) {
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
  const groupId = useGroupStore((state) => state.selectedGroupId);
  const currentActors = useGroupStore((state) => state.actors);
  const activeTab = useUIStore((state) => state.activeTab);
  const [mounted, setMounted] = useState<string[]>([]);
  const [width, setWidth] = useState(innerWidth);
  const { theme, setTheme, isDark: dark } = useTheme();
  const { textScale, setTextScale } = useTextScale();
  const [readOnly, setReadOnly] = useState(false);
  const [canAccessAccount, setCanAccessAccount] = useState(true);
  const [menuOpen, setMenuOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
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
    seed(id);
    setMounted([]);
    useUIStore.getState().setActiveTab("chat");
    useComposerStore.getState().setDestGroupId(id);
  };
  Object.assign(window, {
    groupWorkProbe: {
      ...probe,
      chooseGroup: changeGroup,
      setCount: (count: number) => useGroupStore.setState({ actors: actors.slice(0, count) }),
      patchActor: (id: string, patch: Partial<Actor>) =>
        useGroupStore.setState((state) => ({
          actors: state.actors.map((actor) => (actor.id === id ? { ...actor, ...patch } : actor)),
        })),
      setReadOnly,
      setCanAccessAccount,
      setTextScale,
      setDark: (value: boolean) => {
        setTheme(value ? "dark" : "light");
      },
      language: (lang: string) => i18next.changeLanguage(lang),
      ui: useUIStore,
      group: useGroupStore,
      modals: useModalStore,
    },
  });
  const props = {
    canUseVoice: true,
    canAccessAccount,
    orderedGroups: groups,
    archivedGroupIds: [],
    selectedGroupId: groupId,
    groupDoc: doc(groupId),
    groupContext: { agent_states: [] },
    actors: currentActors,
    runtimeActors: currentActors,
    recipientActors: currentActors,
    recipientActorsBusy: false,
    destGroupScopeLabel: "",
    renderedActorIds: mounted,
    activeTab,
    busy: "",
    isTransitioning: false,
    sidebarOpen: false,
    sidebarCollapsed: false,
    sidebarWidth: 248,
    isDark: dark,
    isSmallScreen: width < 768,
    webReadOnly: readOnly,
    selectedGroupRunning: true,
    selectedGroupRuntimeStatus: null,
    selectedGroupActorsHydrating: false,
    selectedGroupActorStatusProvisional: false,
    theme,
    textScale,
    sseStatus: "connected",
    groupLabelById: { g1: "Release workspace", g2: "Research workspace" },
    mentionSelectedIndex: 0,
    showMentionMenu: false,
    composerRef,
    fileInputRef,
    eventContainerRef,
    contentRef,
    chatAtBottomRef,
    onSelectGroup: changeGroup,
    onTabChange: (tab: string) => {
      useUIStore.getState().setActiveTab(tab);
      if (tab !== "chat") setMounted((ids) => (ids.includes(tab) ? ids : [...ids, tab]));
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
    "onStopGroup",
    "onSetGroupState",
    "onOpenSettings",
    "onOpenAccount",
    "onOpenMobileMenu",
    "appendComposerFiles",
    "setMentionFilter",
    "setMentionKind",
    "setMentionActorScope",
    "setMentionTargetGroupId",
    "setMentionSelectedIndex",
    "setShowMentionMenu",
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
      <MobileMenuSheet
        isOpen={menuOpen}
        onClose={() => setMenuOpen(false)}
        theme={theme}
        textScale={textScale}
        selectedGroupId={groupId}
        groupDoc={doc(groupId)}
        selectedGroupRunning
        actors={currentActors}
        busy=""
        onThemeChange={setTheme}
        onTextScaleChange={setTextScale}
        onOpenSearch={noop}
        onOpenContext={noop}
        onOpenSettings={props.onOpenSettings}
        canAccessAccount={canAccessAccount}
        onOpenAccount={props.onOpenAccount}
        onStartGroup={noop}
        onStopGroup={noop}
        onSetGroupState={noop}
      />
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
