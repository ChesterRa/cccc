// Isolated UI transport. Never contact a live CCCC or model provider.
import { WebModelRuntimePanel } from "../../src/components/webModel/WebModelRuntimePanel";
import type { Actor } from "../../src/types";
import { useGroupStore, useModalStore } from "../../src/stores";
import { useRef, useState } from "react";
import { useActorActions } from "../../src/hooks/useActorActions";
import { AppModals } from "../../src/components/AppModals";
import WebModelConnectorsTab from "../../src/components/modals/settings/WebModelConnectorsTab";
import { WebModelActorSetup } from "../../src/components/webModel/WebModelActorSetup";
import i18n, { i18nReady } from "../../src/i18n";
import { MessageFooter } from "../../src/components/messageBubble/MessageBubbleChrome";
import "../../src/index.css";
const params = new URLSearchParams(location.search);
const isDark = params.get("theme") === "dark";
document.documentElement.classList.add(isDark ? "dark" : "light");
document.documentElement.style.fontSize = `${params.get("scale") || 100}%`;
await i18nReady;
await i18n.changeLanguage(params.get("lang") || "en");
const actors: Actor[] = [
  {
    id: "alpha",
    title: "Web reviewer",
    runtime: "web_model",
    runner: "headless",
    role: "foreman",
    enabled: false,
    capability_autoload: ["pack:analysis"],
  },
  {
    id: "beta",
    title: "CLI reviewer",
    runtime: "codex",
    runner: "pty",
    role: "peer",
    enabled: false,
    command: ["codex", "--model", "fixture"],
  },
];
if (params.get("editor") === "1") {
  useGroupStore.setState({
    selectedGroupId: "g_fixture",
    actors,
    runtimes: [
      { name: "codex", display_name: "Codex CLI", available: true },
      { name: "web_model", display_name: "ChatGPT Web Model", available: true },
    ],
  });
}
const probe = {
  requests: [] as { method: string; path: string; body: Record<string, unknown> }[],
  errors: [] as string[],
  commands: [] as Record<string, unknown>[],
  state: "unpaired",
  boundUrl: "",
  previousUrl: "",
  currentUrl: "https://chatgpt.com/c/alpha",
  pairingId: "pair-alpha",
  rejectConnect: false,
  active: params.get("surface") === "recovery",
  deliveryId: "batch-one",
  deliveryState: params.get("surface") === "recovery" ? "ambiguous" : "",
  resumeBlocked: false,
  enabled: false,
  errorCode: "",
  receive: () => {
    probe.state = "bound";
    probe.boundUrl = probe.currentUrl;
  },
};
Object.assign(window, { webModelProbe: probe });
window.addEventListener("error", (e) => probe.errors.push(e.message));
window.addEventListener("unhandledrejection", (e) => probe.errors.push(String(e.reason)));
// Optional real noVNC transport for isolated Xvfb integration probes.
const vncPort = Number(params.get("vncPort") || 0);
const NativeSocket = window.WebSocket;
const surface = () => ({
  active: probe.active,
  state: probe.active ? "ready" : "idle",
  viewer: { kind: vncPort ? "vnc" : "screencast", vnc: { available: !!vncPort } },
  width: 800,
  height: 600,
});
const status = () => ({
  browser_session: {
    active: probe.active,
    ready: probe.active,
    tab_url: probe.currentUrl,
    conversation_url: probe.boundUrl,
    last_delivery_id: probe.deliveryId,
    last_delivery_status: probe.deliveryState,
    can_resume_delivery: probe.deliveryState === "ambiguous",
  },
  browser_surface: surface(),
  pairing: {
    state: probe.state,
    pairing_id: probe.pairingId,
    actor_enabled: probe.enabled,
    error_code: probe.errorCode,
    url: probe.boundUrl,
    previous_url: probe.previousUrl,
  },
});
window.fetch = async (input, init) => {
  const url = new URL(
    typeof input === "string" ? input : input instanceof URL ? input.href : input.url,
    location.href,
  );
  const method = init?.method || "GET";
  const body = typeof init?.body === "string" ? JSON.parse(init.body) : {};
  probe.requests.push({ method, path: url.pathname, body });
  let result: unknown = status();
  if (url.pathname === "/api/v1/groups/g_fixture/actors") {
    result = { actors };
  } else if (url.pathname === "/api/v1/groups/g_fixture/actors/alpha" && method === "POST") {
    Object.assign(actors[0], body);
    result = { actor: actors[0] };
  } else if (url.pathname.endsWith("/prompts")) {
    result = { help: { content: "" } };
  } else if (url.pathname.endsWith("/profiles")) {
    result = { profiles: [] };
  } else if (url.pathname.endsWith("/resume-delivery")) {
    if (probe.resumeBlocked)
      return Response.json(
        { ok: false, error: { code: "draft_pending", message: "Send or clear the draft first" } },
        { status: 400 },
      );
    probe.deliveryState = "resolved";
    result = status();
  } else if (url.pathname.endsWith("/connectors"))
    result =
      method === "GET"
        ? { connectors: [], requires_reconfiguration: true }
        : {
            connector: {
              connector_id: "shared",
              connector_url: "https://fixture.invalid/mcp/shared",
              connector_url_path_token: "https://fixture.invalid/mcp/shared/token/synthetic",
            },
            secret: "synthetic",
          };
  else if (url.pathname.endsWith("/pairing")) {
    if (body.action === "connect") {
      if (probe.rejectConnect)
        return Response.json(
          { ok: false, error: { code: "pairing_composer_occupied", message: "draft" } },
          { status: 409 },
        );
      probe.pairingId += "-next";
      probe.state = "waiting";
      probe.errorCode = "";
      result = { pairing_id: probe.pairingId, state: "waiting" };
    }
    if (body.action === "cancel" || body.action === "remove") {
      probe.state = body.action === "cancel" ? "cancelled" : "unpaired";
      if (body.action === "remove") probe.boundUrl = "";
      result = {};
    }
  } else if (url.pathname.endsWith("/bind-current")) {
    probe.active = true;
    probe.currentUrl = body.new_chat ? "https://chatgpt.com/" : String(body.conversation_url);
    result = status();
  } else if (url.pathname.endsWith("/stop")) {
    probe.enabled = false;
    result = status();
  } else if (url.pathname.endsWith("/open")) {
    probe.active = true;
    result = status();
  } else if (url.pathname.endsWith("/close")) {
    probe.active = false;
    result = status();
  }
  return Response.json({ ok: true, result });
};

function ActorEditorFixture() {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const { editActor } = useActorActions("g_fixture");
  const currentActors = useGroupStore((state) => state.actors);
  return (
    <>
      <button onClick={() => editActor(currentActors[0])}>Edit Web Actor</button>
      <button onClick={() => editActor(currentActors[1])}>Edit CLI Actor</button>
      <AppModals
        isDark={isDark}
        theme={isDark ? "dark" : "light"}
        textScale={100}
        ccccHome="/fixture/state"
        composerRef={composerRef}
        onStartReply={() => {}}
        onThemeChange={() => {}}
        onTextScaleChange={() => {}}
        onDeleteGroup={async () => {}}
        fetchContext={async () => {}}
        canManageGroups
      />
    </>
  );
}
class FixtureSocket extends EventTarget {
  readyState = 0;
  onopen: ((e: Event) => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onclose: ((e: Event) => void) | null = null;
  constructor(public url: string) {
    super();
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.(new Event("open"));
      this.onmessage?.(
        new MessageEvent("message", { data: JSON.stringify({ t: "state", ...surface() }) }),
      );
    }, 10);
  }
  send(data: string) {
    probe.commands.push(JSON.parse(data));
  }
  close() {
    this.readyState = 3;
  }
  static OPEN = 1;
  static CLOSED = 3;
  static CONNECTING = 0;
}
Object.assign(window, {
  WebSocket: class extends FixtureSocket {
    constructor(url: string) {
      if (vncPort && new URL(url).searchParams.get("mode") === "vnc") {
        return new NativeSocket(`ws://127.0.0.1:${vncPort}/`) as unknown as FixtureSocket;
      }
      super(url);
    }
  },
});
export function Fixture() {
  const [actor, setActor] = useState("");
  const editingActor = useModalStore((state) => state.editingActor);
  const editingSection = useModalStore((state) => state.editingActorSection);
  const currentActors = useGroupStore((state) => state.actors);
  if (params.get("surface") === "runtime")
    return (
      <main className="p-4 flex h-screen flex-col">
        <WebModelRuntimePanel
          groupId="g_fixture"
          actor={params.get("editor") === "1" ? currentActors[0] : actors[0]}
          isDark={isDark}
          isRunning={false}
          isVisible
          readOnly={params.get("readonly") === "1"}
        />
        {editingActor && (
          <output>
            {editingActor.id}/{editingSection}
          </output>
        )}
        {params.get("editor") === "1" && <ActorEditorFixture />}
      </main>
    );
  if (params.get("surface") === "recovery")
    return (
      <main className="p-4 min-h-screen bg-[var(--color-bg-primary)] text-[var(--color-text-primary)]">
        <p className="mb-4">Fixture message to P0</p>
        <MessageFooter
          readOnly={params.get("readonly") === "1"}
          obligationSummary={null}
          visibleReadStatusEntries={[]}
          readPreviewEntries={[]}
          readPreviewOverflow={0}
          displayNameMap={new Map([["P0", "P0"]])}
          isDark={isDark}
          isMail={false}
          replyRequested={false}
          copiedMessageText={false}
          copyableMessageText="Fixture message"
          onCopyMessageText={() => {}}
          onShowRecipients={() => {}}
          onReply={() => {}}
          canReply={false}
          event={{
            v: 1,
            id: "source-one",
            ts: "2026-09-22T14:38:35Z",
            kind: "chat.message",
            group_id: "g_fixture",
            by: "user",
            data: { to: ["P0"], text: "Fixture message" },
          }}
          webModelDeliveryStatus={{
            state: "ambiguous",
            actorId: "P0",
            deliveryId: "batch-one",
            updatedAt: "",
            detail: "",
          }}
        />
      </main>
    );
  return (
    <main className="p-4 min-h-screen bg-[var(--color-bg-primary)] text-[var(--color-text-primary)]">
      <nav className="flex gap-3 mb-4">
        <button onClick={() => setActor("")}>Global</button>
        <button onClick={() => setActor("alpha")}>Actor A</button>
        <button onClick={() => setActor("beta")}>Actor B</button>
      </nav>
      {actor ? (
        <WebModelActorSetup key={actor} groupId="g_fixture" actorId={actor} isDark={isDark} />
      ) : (
        <WebModelConnectorsTab isDark={isDark} />
      )}
    </main>
  );
}
