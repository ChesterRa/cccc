// Real components, layout and visibility observer. No daemon, microphone or model calls.
import { createRef, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import modals from "../../src/i18n/locales/en/modals.json";
import common from "../../src/i18n/locales/en/common.json";
import actors from "../../src/i18n/locales/en/actors.json";
import { CodexVoiceAnalystModal } from "../../src/components/modals/CodexVoiceAnalystModal";
import { useVoiceViewedMessages } from "../../src/features/codexVoice/useVoiceViewedMessages";
import type { CodexVoiceSessionController } from "../../src/features/codexVoice/useCodexVoiceSessionController";
import { useGroupStore } from "../../src/stores/useGroupStore";
import type { GroupMeta } from "../../src/types";
import "../../src/index.css";

await i18next
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { modals, common, actors } } });
const probe = {
  writes: [] as { url: string; body: unknown }[],
  viewed: [] as string[],
  opened: "",
  stopped: false,
};
Object.assign(window, { voiceDuplexProbe: probe });
let preferences = {
  revision: 0,
  groups: {},
  verbosity: "standard",
  style: "natural",
  suppress_viewed: true,
};
window.fetch = async (input, init) => {
  const url = String(input);
  const body = init?.body ? JSON.parse(String(init.body)) : {};
  if (init?.method && init.method !== "GET") probe.writes.push({ url, body });
  if (url === "/api/v1/codex_voice/preferences") {
    if (init?.method === "PUT")
      preferences = { ...body.preferences, revision: preferences.revision + 1 };
    return Response.json({ ok: true, result: { preferences } });
  }
  if (url === "/api/v1/codex_voice/notifications")
    return Response.json({
      ok: true,
      result: {
        pending_count: 0,
        unconfirmed_count: 1,
        suppressed_count: 1,
        messages: [
          {
            sequence: 0,
            source: { group_id: "g_fixture", event_id: "e_skipped" },
            by: "worker",
            processed: true,
            attempted: true,
            output_status: "suppressed",
            suppression_reason: "viewed",
          },
          {
            sequence: 0,
            source: { group_id: "g_fixture", event_id: "e_fixture" },
            by: "worker",
            output_status: "unconfirmed",
            processed: true,
            attempted: false,
          },
        ],
      },
    });
  if (url === "/api/v1/codex_voice/messages/viewed") {
    probe.viewed.push(...body.messages.map((source: { event_id: string }) => source.event_id));
    return Response.json({ ok: true, result: { observed: body.messages.length } });
  }
  if (url === "/api/v1/codex_voice/analyst-settings")
    return Response.json({
      ok: true,
      result: {
        settings: {
          runtime: "codex",
          command: [],
          profile_id: "",
          profile_scope: "global",
          profile_owner: "",
        },
        environment_keys: [],
      },
    });
  if (url.startsWith("/api/v1/profiles"))
    return Response.json({ ok: true, result: { profiles: [] } });
  throw new Error(`Unexpected isolated fixture request: ${url}`);
};
useGroupStore.setState({
  groups: [
    { group_id: "g_fixture", title: "Release coordination" },
    { group_id: "g_second", title: "Long project title for narrow-screen layout verification" },
  ] as GroupMeta[],
});
const controller = {
  audioRef: createRef<HTMLAudioElement>(),
  phase: "listening",
  call: null,
  analyst: {
    generation: "analyst",
    tui_ready: false,
    phase: "ready",
    last_result: "",
    warning: "",
  },
  owned: true,
  checking: false,
  conversation: [
    { id: "user-1", role: "user", text: "Tell me when the release tests finish.", final: true },
    {
      id: "assistant-1",
      role: "assistant",
      text: "The first test run passed. I am waiting for the final result.",
      final: true,
    },
    {
      id: "assistant-2",
      role: "assistant",
      text: "The final test run has also passed. The worker reports that the package is ready for review.",
      final: true,
    },
  ],
  notificationPaused: false,
  microphoneMuted: false,
  playbackBlocked: false,
  outputStatus: { queued: 1, blocked: "conversation" },
  error: "",
  isStarting: false,
  isEngaged: true,
  externalCall: false,
  analystWorking: false,
  analystWarning: "",
  preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
  supportedVoices: ["cove", "sol"],
  readiness: {
    analyst_runtime: "codex",
    analyst_runtime_available: true,
    realtime_credentials_available: true,
  },
  updatePreferences: () => {},
  refresh: async () => {},
  start: async () => {},
  disconnect: async () => {
    probe.stopped = true;
  },
  cancelInvestigation: async () => true,
  toggleMicrophone: () => {},
  resumeAudio: async () => {},
  startNewAnalyst: async () => true,
  clearError: () => {},
} as CodexVoiceSessionController;

export function Fixture() {
  const [open, setOpen] = useState(true);
  const [dark, setDark] = useState(false);
  const [session, setSession] = useState(controller);
  Object.assign(probe, {
    update: (patch: Partial<CodexVoiceSessionController>) =>
      setSession((current) => ({ ...current, ...patch })),
    setDark: (value: boolean) => {
      document.documentElement.classList.toggle("dark", value);
      setDark(value);
    },
  });
  const root = useRef<HTMLDivElement>(null);
  useVoiceViewedMessages(root, "g_fixture", true);
  return (
    <>
      <button onClick={() => setOpen(true)}>Open Voice</button>
      <div ref={root} style={{ padding: 30 }}>
        <div
          data-voice-viewable="true"
          data-message-id="visible"
          style={{ padding: 10, width: 250 }}
        >
          Visible complete message
        </div>
        <div data-voice-viewable="true" data-message-id="folded">
          <details>
            <summary>Hidden message</summary>Not yet read
          </details>
        </div>
        <div
          data-voice-viewable="true"
          data-message-id="overscan"
          style={{ position: "absolute", top: 3000 }}
        >
          Outside the viewport
        </div>
      </div>
      <CodexVoiceAnalystModal
        isOpen={open}
        isDark={dark}
        isSmallScreen={window.innerWidth < 768}
        controller={session}
        onClose={() => setOpen(false)}
        onOpenSource={(group, event) => {
          probe.opened = `${group}:${event}`;
        }}
      />
    </>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
