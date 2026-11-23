// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import type { AcpPermission } from "../../services/api/codexVoice";
import { CodexVoiceDock } from "./CodexVoiceDock";

const unattended = vi.hoisted(() => ({ enabled: false, refresh: vi.fn(async () => undefined) }));
vi.mock("../../stores/useUnattendedStore", () => ({
  useUnattendedStore: (select: (state: unknown) => unknown) =>
    select({
      unattended: unattended.enabled ? { enabled: true, since: "t" } : null,
      refresh: unattended.refresh,
    }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function controller(
  overrides: Partial<CodexVoiceSessionController> = {},
): CodexVoiceSessionController {
  return {
    audioDeviceSnapshot: null,
    audioRef: createRef<HTMLAudioElement>(),
    phase: "idle",
    call: null,
    analyst: null,
    owned: false,
    checking: false,
    conversation: [],
    notificationPaused: false,
    microphoneMuted: false,
    playbackBlocked: false,
    outputStatus: { queued: 0, blocked: null },
    error: "",
    recoveryOwners: [],
    refreshError: "",
    isStarting: false,
    isEngaged: false,
    externalCall: false,
    analystWorking: false,
    analystWarning: "",
    preferences: { voice: "cove", inputDeviceId: "", outputDeviceId: "" },
    supportedVoices: ["cove"],
    readiness: {
      analyst_runtime: "codex",
      analyst_runtime_available: true,
      supported_modes: ["assistant", "persona"],
      realtime_credentials_available: true,
    },
    updatePreferences: vi.fn(),
    updateAnalystSnapshot: vi.fn(),
    refresh: vi.fn(async () => undefined),
    start: vi.fn(async () => undefined),
    disconnect: vi.fn(async () => undefined),
    cancelInvestigation: vi.fn(async () => true),
    toggleMicrophone: vi.fn(),
    resumeAudio: vi.fn(async () => undefined),
    startNewAnalyst: vi.fn(async () => true),
    clearError: vi.fn(),
    ...overrides,
  };
}

describe("CodexVoiceDock", () => {
  it.each(["sidebar", "mobile", "collapsed"] as const)(
    "marks something new in Chet's history on the %s entry, naming it only in its tooltip",
    (surface) => {
      const markup = (unread: boolean) =>
        renderToStaticMarkup(
          <CodexVoiceDock
            controller={controller()}
            chetUnread={unread}
            variant={surface === "mobile" ? "mobile" : "sidebar"}
            collapsed={surface === "collapsed"}
            onOpen={vi.fn()}
            onStart={vi.fn()}
          />,
        );
      expect(markup(false)).not.toContain("data-chet-unread");
      const unread = markup(true);
      expect(unread).toContain("data-chet-unread");
      expect(unread).toContain('aria-label="modals:chet.open · modals:chet.unread"');
      expect(unread).toContain("Chet · modals:chet.subtitle · modals:chet.unread");
    },
  );

  it.each(["sidebar", "mobile", "collapsed"] as const)(
    "shows Chet's head in the %s dock, moving only while Chet itself works",
    (surface) => {
      const markup = (overrides: Partial<CodexVoiceSessionController>, chetWorking = false) =>
        renderToStaticMarkup(
          <CodexVoiceDock
            controller={controller(overrides)}
            chetWorking={chetWorking}
            variant={surface === "mobile" ? "mobile" : "sidebar"}
            collapsed={surface === "collapsed"}
            onOpen={vi.fn()}
            onStart={vi.fn()}
          />,
        );
      expect(markup({})).toContain('data-chet-head="idle"');
      expect(markup({}, true)).toContain('data-chet-head="working"');
      expect(markup({ analystWorking: true })).toContain('data-chet-head="working"');
      // A live call is not Chet's work: the call controls show it and the head stays idle.
      expect(markup({ phase: "listening", owned: true, isEngaged: true })).toContain(
        'data-chet-head="idle"',
      );
      // Something that needs a look takes the head's place with the alert mark.
      expect(markup({ error: "Needs attention" })).not.toContain("data-chet-head");
    },
  );

  it.each(["sidebar", "mobile", "collapsed"] as const)(
    "names Chet, not the next call's service, in the %s dock before a call",
    (surface) => {
      const html = renderToStaticMarkup(
        <CodexVoiceDock
          controller={controller({
            preferences: { provider: "grok", voice: "eve", inputDeviceId: "", outputDeviceId: "" },
          })}
          variant={surface === "mobile" ? "mobile" : "sidebar"}
          collapsed={surface === "collapsed"}
          onOpen={vi.fn()}
          onStart={vi.fn()}
        />,
      );
      expect(html).toContain("modals:chet.subtitle");
      expect(html).not.toContain("modals:codexVoiceProviders.grok");
      expect(html).not.toContain("Eve");
      if (surface !== "collapsed") expect(html).toMatch(/>Chet<\/span>/);
    },
  );

  it.each(["sidebar", "mobile", "collapsed"] as const)(
    "names unattended mode in the %s dock on its own line beside Chet's own state",
    (surface) => {
      const markup = (chetWorking = false, overrides: Partial<CodexVoiceSessionController> = {}) =>
        renderToStaticMarkup(
          <CodexVoiceDock
            controller={controller(overrides)}
            chetWorking={chetWorking}
            variant={surface === "mobile" ? "mobile" : "sidebar"}
            collapsed={surface === "collapsed"}
            onOpen={vi.fn()}
            onStart={vi.fn()}
          />,
        );
      unattended.enabled = true;
      try {
        const idle = markup();
        expect(idle).toContain("modals:chet.unattendedOn");
        const working = markup(true);
        expect(working).toContain("modals:chet.unattendedOn · modals:chet.state.working");
        // A call or something that needs a look keeps the mode named beside it.
        const attention = markup(false, { error: "Needs attention" });
        expect(attention).toContain("modals:chet.unattendedOn · layout:codexVoiceAttention");
        expect(markup(false, { phase: "listening", owned: true, isEngaged: true })).toContain(
          "modals:chet.unattendedOn",
        );
        // Copies would wait for a Chet that cannot run, so the entry asks for a look.
        const cannotRun = {
          readiness: { ...controller().readiness!, analyst_runtime_available: false },
        };
        const runtimeAttention = markup(false, cannotRun);
        expect(runtimeAttention).toContain("modals:chet.unattendedOn · layout:codexVoiceAttention");
        if (surface !== "collapsed") {
          // Visible docks keep unattended on its own non-truncated row so states never clip it.
          expect(runtimeAttention).toMatch(
            /<span data-chet-unattended="true" class="[^"]*">modals:chet\.unattendedOn<\/span>/,
          );
          expect(runtimeAttention).not.toMatch(
            /<span data-chet-unattended="true" class="[^"]*truncate[^"]*">/,
          );
          expect(runtimeAttention).toMatch(/>layout:codexVoiceAttention<\/span>/);
          expect(idle).not.toContain("modals:chet.subtitle");
        }
      } finally {
        unattended.enabled = false;
      }
      expect(markup()).not.toContain("modals:chet.unattendedOn");
      expect(markup()).not.toContain("data-chet-unattended");
      // Without the mode, a Runtime problem stays inside Chet's panel as before.
      expect(
        markup(false, {
          readiness: { ...controller().readiness!, analyst_runtime_available: false },
        }),
      ).not.toContain("layout:codexVoiceAttention");
    },
  );

  it("keeps the live call identity when next-call preferences differ", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({
          phase: "idle",
          isEngaged: true,
          externalCall: true,
          call: {
            provider: "grok",
            voice: "aurora",
            generation: "call-1",
            analyst_generation: null,
            mode: "assistant",
            connected: true,
          },
          preferences: { provider: "codex", voice: "cove", inputDeviceId: "", outputDeviceId: "" },
        })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );
    expect(html).toContain("modals:codexVoiceProviders.grok");
    expect(html).toContain("Aurora");
    expect(html).toContain("modals:codexVoiceActiveElsewhere");
    expect(html).not.toContain("Cove");
  });

  it("shows stale status instead of advertising ready after a failed snapshot read", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({ refreshError: "read failed" })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );
    expect(html).toContain("modals:codexVoiceStatusStaleLabel");
    expect(html).not.toContain("modals:codexVoicePhase.failed");
    expect(html).not.toContain("modals:codexVoicePhase.idle");
  });

  it("keeps Chet available when only the voice call is not ready", () => {
    const state = controller();
    state.readiness = { ...state.readiness!, realtime_credentials_available: false };
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={state} onOpen={vi.fn()} onStart={vi.fn()} />,
    );
    // Text needs no Realtime credentials, so a voice problem does not mark Chet as blocked.
    expect(html).toContain("modals:chet.subtitle");
    expect(html).not.toContain("modals:codexVoiceCallBlocked");
    expect(html).not.toContain("modals:codexVoicePhase.idle");
  });

  it("shows a queued or running warm investigation instead of Analyst ready", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({
          analyst: {
            generation: "A",
            phase: "ready",
            tui_ready: false,
            last_result: "",
            warning: "",
          },
          analystWorking: true,
        })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );
    expect(html).toContain("modals:chet.state.working");
    expect(html).not.toContain("layout:codexVoiceAnalystReady");
  });

  it("claims nothing about readiness before the first snapshot", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({ readiness: null })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );
    expect(html).toContain("modals:chet.subtitle");
    expect(html).not.toContain("modals:codexVoicePhase.idle");
  });

  it("shows Chet's own work while no Analyst snapshot reports it", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={controller()} chetWorking onOpen={vi.fn()} onStart={vi.fn()} />,
    );
    expect(html).toContain("modals:chet.state.working");
    expect(html).toContain('data-chet-head="working"');
  });

  it.each(["sidebar", "mobile", "collapsed"] as const)(
    "marks %s dock as needing attention when Analyst is working but waiting on a permission approval",
    (surface) => {
      const renderDock = (permissions: AcpPermission[]) =>
        renderToStaticMarkup(
          <CodexVoiceDock
            controller={controller({
              analystWorking: true,
              analyst: {
                generation: "analyst-perm",
                tui_ready: false,
                phase: "working",
                last_result: "",
                warning: "",
                structured: true,
                permissions,
              },
            })}
            chetWorking
            variant={surface === "mobile" ? "mobile" : "sidebar"}
            collapsed={surface === "collapsed"}
            onOpen={vi.fn()}
            onStart={vi.fn()}
          />,
        );

      unattended.enabled = true;
      try {
        const waiting = renderDock([
          { request_id: "perm-1", kind: "permission", title: "Run command" },
        ]);
        expect(waiting).toContain("modals:chet.unattendedOn · layout:codexVoiceAttention");
        expect(waiting).not.toContain("modals:chet.state.working");
        expect(waiting).not.toContain("data-chet-head");

        const resumed = renderDock([]);
        expect(resumed).toContain("modals:chet.unattendedOn · modals:chet.state.working");
        expect(resumed).not.toContain("layout:codexVoiceAttention");
        expect(resumed).toContain('data-chet-head="working"');
      } finally {
        unattended.enabled = false;
      }

      const waitingWithoutUnattended = renderDock([
        { request_id: "perm-1", kind: "permission", title: "Run command" },
      ]);
      expect(waitingWithoutUnattended).toContain("layout:codexVoiceAttention");
      expect(waitingWithoutUnattended).not.toContain("data-chet-head");
    },
  );

  it("keeps two stable global actions: open console and start voice", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={controller()} onOpen={vi.fn()} onStart={vi.fn()} />,
    );

    expect(html).toContain('aria-label="modals:chet.open"');
    expect(html).toContain('aria-label="layout:codexVoiceStart"');
    expect(html).toContain("lucide-audio-lines");
    expect(html).not.toContain("lucide-mic");
    expect(html).not.toContain("g_alpha");
    expect(html).not.toContain("Settings");
  });

  it("changes only the call control to stop while voice is active", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({ phase: "listening", owned: true, isEngaged: true })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="modals:chet.open"');
    expect(html).toContain('aria-label="modals:codexVoiceStop"');
    expect(html).not.toContain("modals:codexVoiceMute");
  });

  it("preserves the same two semantics in the collapsed sidebar", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock controller={controller()} collapsed onOpen={vi.fn()} onStart={vi.fn()} />,
    );

    expect(html).toContain('aria-label="modals:chet.open"');
    expect(html).toContain('aria-label="layout:codexVoiceStart"');
  });

  it("reports the global assistant without exposing Group state", () => {
    const html = renderToStaticMarkup(
      <CodexVoiceDock
        controller={controller({
          analyst: {
            generation: "analyst-1",
            tui_ready: true,
            phase: "ready",
            last_result: "",
            warning: "",
          },
        })}
        onOpen={vi.fn()}
        onStart={vi.fn()}
      />,
    );

    expect(html).toContain("modals:chet.subtitle");
    expect(html).not.toContain("Alpha");
    expect(html).not.toContain("g_beta");
  });

  it("reads unattended mode again when the page becomes visible", () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    unattended.refresh.mockClear();
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    act(() =>
      root.render(<CodexVoiceDock controller={controller()} onOpen={vi.fn()} onStart={vi.fn()} />),
    );
    expect(unattended.refresh).toHaveBeenCalledTimes(1);
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(unattended.refresh).toHaveBeenCalledTimes(2);
    act(() => root.unmount());
    document.dispatchEvent(new Event("visibilitychange"));
    expect(unattended.refresh).toHaveBeenCalledTimes(2);
    host.remove();
  });
});
