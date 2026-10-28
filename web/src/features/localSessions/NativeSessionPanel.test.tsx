// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, LocalSessionRecovery } from "../../types";
import { useGroupStore } from "../../stores/useGroupStore";
import enActors from "../../i18n/locales/en/actors.json";
import enCommon from "../../i18n/locales/en/common.json";
import zhActors from "../../i18n/locales/zh/actors.json";
import zhCommon from "../../i18n/locales/zh/common.json";
import jaActors from "../../i18n/locales/ja/actors.json";
import jaCommon from "../../i18n/locales/ja/common.json";

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  refresh: vi.fn(),
  refreshActors: vi.fn(),
  clipboard: vi.fn(),
  fallback: vi.fn(),
}));
vi.mock("../../services/api/localSessions", () => ({
  fetchLocalSessionRecovery: mocks.load,
  refreshLocalSessionRecovery: mocks.refresh,
}));
import { NativeSessionPanel } from "./NativeSessionPanel";

type Props = ComponentProps<typeof NativeSessionPanel>;
const GROUP_ID = "g_original";
const nativeActor: Actor = {
  id: "native",
  generation: "generation-original",
  runtime: "codex",
  external_session_id: "codex:native-thread-from-server",
  enabled: false,
  running: false,
};
const originalCwd = "/synthetic/original workspace/日本";
const originalCommand =
  "cd -- '/synthetic/original workspace/日本' && CCCC_HOME='/synthetic/cccc home' CODEX_HOME='/synthetic/codex home' codex resume 'native-thread-from-server'";
const locales = {
  en: { actors: enActors, common: enCommon },
  zh: { actors: zhActors, common: zhCommon },
  ja: { actors: jaActors, common: jaCommon },
};
const i18n = createInstance();
const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
const execCommandDescriptor = Object.getOwnPropertyDescriptor(document, "execCommand");
let host: HTMLDivElement;
let root: Root;

function recoveryResponse(
  metadata: Partial<LocalSessionRecovery["recovery"]> = {},
  actor = nativeActor,
  groupId = GROUP_ID,
) {
  return {
    ok: true as const,
    result: {
      group_id: groupId,
      actor_id: actor.id,
      source_id: actor.external_session_id || "",
      refresh_status: "checked" as "checked" | "unavailable" | "not_checked",
      recovery: {
        state: "disconnected" as LocalSessionRecovery["recovery"]["state"],
        cwd: originalCwd,
        shell: "posix" as const,
        command: originalCommand,
        ...metadata,
      },
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.resetAllMocks();
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      defaultNS: "actors",
      resources: locales,
      interpolation: { escapeValue: false },
    });
  vi.useFakeTimers();
  useGroupStore.setState({ groupDoc: null, refreshActors: mocks.refreshActors });
  mocks.refreshActors.mockResolvedValue(undefined);
  mocks.load.mockResolvedValue(recoveryResponse());
  mocks.refresh.mockResolvedValue(recoveryResponse({ state: "connected" }));
  mocks.clipboard.mockResolvedValue(undefined);
  mocks.fallback.mockReturnValue(false);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: mocks.clipboard },
    configurable: true,
  });
  Object.defineProperty(document, "execCommand", { value: mocks.fallback, configurable: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
  if (execCommandDescriptor) Object.defineProperty(document, "execCommand", execCommandDescriptor);
  else Reflect.deleteProperty(document, "execCommand");
  vi.useRealTimers();
});

async function render(patch: Partial<Props> = {}) {
  const props: Props = {
    actor: nativeActor,
    groupId: GROUP_ID,
    statusText: "CCCC access paused",
    isVisible: true,
    canAccessRecovery: true,
    ...patch,
  };
  await act(async () =>
    root.render(
      <I18nextProvider i18n={i18n}>
        <NativeSessionPanel {...props} />
      </I18nextProvider>,
    ),
  );
}

async function clickButton(label: string) {
  const button = [...host.querySelectorAll("button")].find((item) => item.textContent === label);
  expect(button).toBeDefined();
  expect(button!.disabled).toBe(false);
  await act(async () => button!.click());
}

async function advance(milliseconds: number) {
  await act(async () => vi.advanceTimersByTimeAsync(milliseconds));
}

async function expand(open: boolean) {
  const details = host.querySelector("details")!;
  await act(async () => {
    if (details.open !== open) details.querySelector("summary")!.click();
  });
}

it.each(["en", "zh", "ja"] as const)(
  "displays and copies both providers' server-returned commands and directories unchanged in %s",
  async (language) => {
    await act(async () => i18n.changeLanguage(language));
    const text = locales[language].actors.nativeSession.recovery;
    for (const fixture of [
      { actor: nativeActor, cwd: originalCwd, command: originalCommand, shell: "posix" as const },
      {
        actor: {
          ...nativeActor,
          runtime: "claude",
          external_session_id: "claude:host-not-conversation",
        },
        cwd: "/synthetic/claude worktree",
        shell: "posix" as const,
        command:
          "cd -- '/synthetic/claude worktree' && CCCC_HOME='/synthetic/cccc home' CLAUDE_CONFIG_DIR='/synthetic/claude config' claude --resume 'actual-conversation-id'",
      },
      {
        actor: nativeActor,
        cwd: "C:\\synthetic\\original workspace\\日本",
        shell: "powershell" as const,
        command:
          "Set-Location -LiteralPath 'C:\\synthetic\\original workspace\\日本'; $env:CCCC_HOME = 'C:\\synthetic\\cccc'; $env:CODEX_HOME = 'C:\\synthetic\\codex'; codex resume 'native-thread-from-server'",
      },
    ]) {
      mocks.load.mockResolvedValueOnce(recoveryResponse(fixture, fixture.actor));
      await render({ actor: fixture.actor });
      expect(mocks.load).toHaveBeenLastCalledWith(GROUP_ID, fixture.actor.id);
      expect(host.querySelector("section")?.getAttribute("aria-label")).toBe(text.title);
      expect(host.querySelector("pre code")?.textContent).toBe(fixture.command);
      expect(
        [...host.querySelectorAll("code")].some((code) => code.textContent === fixture.cwd),
      ).toBe(true);
      expect(host.querySelector("pre")?.classList.contains("whitespace-pre-wrap")).toBe(true);
      expect(host.querySelector("pre")?.classList.contains("break-all")).toBe(true);
      const shell = fixture.shell === "posix" ? "Bash/zsh" : "PowerShell";
      expect(host.textContent).toContain(text.commandLabel.replace("{{shell}}", shell));
      expect(host.textContent).toContain(text.runWarning.replace("{{shell}}", shell));
      expect(host.textContent).not.toContain("WSL");
      expect(host.textContent).toContain(text.admissionHint);
      expect(host.textContent).toContain(
        fixture.actor.runtime === "codex" ? text.codexReconnect : text.claudeReconnect,
      );
      expect(host.textContent).not.toContain("nativeSession.recovery.");
      expect(host.querySelector("pre code")?.textContent).not.toContain(
        fixture.actor.external_session_id,
      );
      await clickButton(text.copyCommand);
      expect(mocks.clipboard).toHaveBeenLastCalledWith(fixture.command);
      expect(host.textContent).toContain(locales[language].common.copied);
      await clickButton(text.copyDirectory);
      expect(mocks.clipboard).toHaveBeenLastCalledWith(fixture.cwd);
      expect(host.querySelector('[role="alert"]')).toBeNull();
    }
    expect(mocks.load).toHaveBeenCalledTimes(3);
    expect(mocks.refresh).not.toHaveBeenCalled();
  },
);

it.each([
  "connected",
  "disconnected",
  "host_exited",
  "session_ended",
  "integration_disabled",
  "unknown",
] as const)("explains the server's precise %s state independently from access", async (state) => {
  mocks.load.mockResolvedValue(recoveryResponse({ state }));
  await render();
  expect(host.querySelector('[role="status"]')?.textContent).toBe("CCCC access paused");
  expect(host.textContent).toContain(enActors.nativeSession.recovery.states[state]);
  expect(host.textContent).toContain(enActors.nativeSession.recovery.hints[state]);
  expect(host.textContent).toContain(enActors.nativeSession.recovery.admissionHint);
  expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
});

it.each(["CCCC access paused", "Group paused", "Group stopped", "Available to receive"])(
  "keeps %s separate from a connected native integration",
  async (statusText) => {
    mocks.load.mockResolvedValue(recoveryResponse({ state: "connected" }));
    await render({ statusText });
    expect(host.querySelector('[role="status"]')?.textContent).toBe(statusText);
    expect(host.textContent).toContain("Receive integration connected");
    expect(host.textContent).toContain(enActors.nativeSession.recovery.hints.connected);
    expect(host.textContent).toContain(enActors.nativeSession.recovery.admissionHint);
    expect(host.textContent).not.toContain("Native host exited");
  },
);

it("shows unknown metadata and the server's Claude picker without substituting a source/host ID", async () => {
  const actor = { ...nativeActor, runtime: "claude", external_session_id: "claude:original-host" };
  const command = "CCCC_HOME='/synthetic/cccc' claude --resume";
  mocks.load.mockResolvedValue(recoveryResponse({ state: "unknown", cwd: null, command }, actor));
  await render({ actor });
  expect(host.textContent).toContain("Native state unknown");
  expect(host.textContent).toContain(enActors.nativeSession.recovery.hints.unknown);
  expect(host.textContent).toContain(enActors.nativeSession.recovery.claudeReconnect);
  expect(host.querySelector("pre code")?.textContent).toBe(command);
  expect(host.textContent).toContain(enActors.nativeSession.recovery.cwdUnknown);
  expect(
    [...host.querySelectorAll("button")].some((button) => button.textContent === "Copy directory"),
  ).toBe(false);
  expect(host.textContent).not.toContain("Native host exited");
  expect(host.textContent).not.toContain("Native session ended");
  await clickButton("Copy command");
  expect(mocks.clipboard).toHaveBeenCalledExactlyOnceWith(command);
});

it("announces loading without inventing a lifecycle state or a command", async () => {
  const pending = deferred<ReturnType<typeof recoveryResponse>>();
  mocks.load.mockReturnValueOnce(pending.promise);
  await render();
  expect(host.querySelector('section[aria-busy="true"]')).not.toBeNull();
  expect(host.textContent).toContain("Loading...");
  expect(host.querySelector("pre")).toBeNull();
  expect(host.textContent).not.toContain("Native state unknown");
  expect(host.textContent).not.toContain("Native host exited");
  await act(async () => pending.resolve(recoveryResponse()));
  expect(host.querySelector('section[aria-busy="true"]')).toBeNull();
  expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
});

it.each(["pi", "opencode"])("keeps %s recovery in the original application", async (runtime) => {
  const actor = { ...nativeActor, runtime, external_session_id: `${runtime}:original-host` };
  const command =
    runtime === "pi"
      ? "PI_CODING_AGENT_DIR='/synthetic/pi' pi --session 'original-session'"
      : "XDG_CONFIG_HOME='/synthetic/config' opencode --session 'ses_original'";
  mocks.load.mockResolvedValue(recoveryResponse({ state: "session_ended", command }, actor));
  await render({ actor });
  expect(host.textContent).toContain(
    runtime === "pi"
      ? enActors.nativeSession.recovery.piReconnect
      : enActors.nativeSession.recovery.opencodeReconnect,
  );
  expect(host.textContent).not.toContain(enActors.nativeSession.recovery.claudeReconnect);
  expect(host.querySelector("pre code")?.textContent).toBe(command);
  await clickButton("Copy command");
  expect(mocks.clipboard).toHaveBeenCalledExactlyOnceWith(command);
});

it.each(["API error", "rejected read"])(
  "reports %s honestly and offers a local retry",
  async (failure) => {
    if (failure === "API error") {
      mocks.load.mockResolvedValueOnce({
        ok: false,
        error: { code: "read_failed", message: "Synthetic failure" },
      });
    } else mocks.load.mockRejectedValueOnce(new Error("Synthetic failure"));
    await render();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      enActors.nativeSession.recovery.loadFailed,
    );
    expect(host.querySelector("pre")).toBeNull();
    expect(host.textContent).not.toContain("Native host exited");
    expect(host.textContent).not.toContain("Native state unknown");
    await advance(12000);
    expect(mocks.load).toHaveBeenCalledOnce();
    await clickButton("Refresh status");
    expect(mocks.load).toHaveBeenCalledOnce();
    expect(mocks.refresh).toHaveBeenCalledOnce();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
  },
);

it("removes previously displayed recovery details when a refresh fails", async () => {
  mocks.load
    .mockResolvedValueOnce(recoveryResponse({ state: "connected" }))
    .mockResolvedValueOnce({
      ok: false,
      error: { code: "read_failed", message: "Synthetic refresh failure" },
    });
  await render();
  expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
  await render({ actor: { ...nativeActor, enabled: true } });
  expect(host.querySelector("pre")).toBeNull();
  expect(host.textContent).not.toContain(originalCwd);
  expect(host.textContent).not.toContain("Receive integration connected");
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(
    enActors.nativeSession.recovery.loadFailed,
  );
  await clickButton("Refresh status");
  expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
});

it.each([
  { name: "hidden pane", patch: { isVisible: false } },
  { name: "non-administrator", patch: { canAccessRecovery: false } },
  { name: "hidden unauthorized pane", patch: { isVisible: false, canAccessRecovery: false } },
  { name: "missing Group", patch: { groupId: "" } },
  { name: "missing binding", patch: { actor: { ...nativeActor, external_session_id: undefined } } },
])("never fetches or exposes recovery for a $name", async ({ patch }) => {
  await render(patch);
  expect(mocks.load).not.toHaveBeenCalled();
  expect(host.querySelector("section")).toBeNull();
  expect(host.querySelector("pre")).toBeNull();
  expect(host.querySelector('[role="status"]')?.textContent).toBe("CCCC access paused");
  await advance(9000);
  expect(mocks.load).not.toHaveBeenCalled();
  await render();
  expect(mocks.load).toHaveBeenCalledExactlyOnceWith(GROUP_ID, nativeActor.id);
});

it.each([
  { name: "Actor", patch: { actor: { ...nativeActor, id: "other-actor" } } },
  { name: "Group", patch: { groupId: "g_other" } },
  {
    name: "source",
    patch: { actor: { ...nativeActor, external_session_id: "codex:other-thread" } },
  },
  { name: "generation", patch: { actor: { ...nativeActor, generation: "generation-replaced" } } },
  {
    name: "provider",
    patch: {
      actor: { ...nativeActor, runtime: "claude", external_session_id: "claude:other-host" },
    },
  },
])("clears old details and ignores a late read across a $name change", async ({ patch }) => {
  const oldRead = deferred<ReturnType<typeof recoveryResponse>>();
  const freshRead = deferred<ReturnType<typeof recoveryResponse>>();
  mocks.load
    .mockResolvedValueOnce(recoveryResponse())
    .mockReturnValueOnce(oldRead.promise)
    .mockReturnValueOnce(freshRead.promise);
  await render();
  expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
  await render({ actor: { ...nativeActor, enabled: true } });
  expect(mocks.load).toHaveBeenCalledTimes(2);
  await render(patch);
  expect(mocks.load).toHaveBeenCalledTimes(3);
  expect(host.querySelector("pre")).toBeNull();
  expect(host.textContent).not.toContain(originalCwd);
  const actor = patch.actor ?? nativeActor;
  const groupId = patch.groupId ?? GROUP_ID;
  const fresh = recoveryResponse(
    { command: "server-returned fresh command", cwd: "/synthetic/fresh" },
    actor,
    groupId,
  );
  await act(async () => freshRead.resolve(fresh));
  await act(async () =>
    oldRead.resolve(recoveryResponse({ command: "late old command", cwd: "/synthetic/old" })),
  );
  expect(host.querySelector("pre code")?.textContent).toBe(fresh.result.recovery.command);
  expect(host.textContent).not.toContain("/synthetic/old");
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => root.render(null));
  expect(vi.getTimerCount()).toBe(0);
});

it.each([
  { name: "visibility", patch: { isVisible: false } },
  { name: "administrator permission", patch: { canAccessRecovery: false } },
])("drops private details and late results when $name is lost and restored", async ({ patch }) => {
  const oldRead = deferred<ReturnType<typeof recoveryResponse>>();
  const freshRead = deferred<ReturnType<typeof recoveryResponse>>();
  mocks.load
    .mockResolvedValueOnce(recoveryResponse())
    .mockReturnValueOnce(oldRead.promise)
    .mockReturnValueOnce(freshRead.promise);
  await render();
  await render({ actor: { ...nativeActor, enabled: true } });
  await render(patch);
  expect(host.querySelector("section")).toBeNull();
  expect(host.textContent).not.toContain(originalCwd);
  expect(vi.getTimerCount()).toBe(0);
  await advance(9000);
  expect(mocks.load).toHaveBeenCalledTimes(2);
  await render();
  expect(mocks.load).toHaveBeenCalledTimes(3);
  await act(async () => oldRead.resolve(recoveryResponse()));
  expect(host.querySelector("pre")).toBeNull();
  expect(host.textContent).not.toContain(originalCwd);
  await act(async () =>
    freshRead.resolve(recoveryResponse({ command: "fresh authorized command" })),
  );
  expect(host.querySelector("pre code")?.textContent).toBe("fresh authorized command");
});

it.each([{ group_id: "g_wrong" }, { actor_id: "wrong-actor" }, { source_id: "claude:wrong-host" }])(
  "does not display a response for a different binding: %j",
  async (mismatch) => {
    const response = recoveryResponse();
    mocks.load.mockResolvedValue({ ...response, result: { ...response.result, ...mismatch } });
    await render();
    expect(host.querySelector("pre")).toBeNull();
    expect(host.textContent).not.toContain(originalCwd);
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      enActors.nativeSession.recovery.loadFailed,
    );
  },
);

it("never schedules periodic reads, including while a read is slow, and ignores late results on unmount", async () => {
  const first = deferred<ReturnType<typeof recoveryResponse>>();
  const second = deferred<ReturnType<typeof recoveryResponse>>();
  mocks.load.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  await render();
  await advance(15000);
  expect(mocks.load).toHaveBeenCalledOnce();
  await act(async () => first.resolve(recoveryResponse()));
  expect(vi.getTimerCount()).toBe(0);
  await advance(60000);
  expect(mocks.load).toHaveBeenCalledOnce();
  await render({ actor: { ...nativeActor, enabled: true } });
  expect(mocks.load).toHaveBeenCalledTimes(2);
  await advance(15000);
  expect(mocks.load).toHaveBeenCalledTimes(2);
  await act(async () => root.render(null));
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => second.resolve(recoveryResponse()));
  expect(vi.getTimerCount()).toBe(0);
  await advance(9000);
  expect(mocks.load).toHaveBeenCalledTimes(2);
  expect(mocks.refresh).not.toHaveBeenCalled();
});

it("defaults connected guidance to collapsed and offline guidance to expanded; opening does not make requests", async () => {
  mocks.load.mockResolvedValueOnce(recoveryResponse({ state: "connected" }));
  await render({ actor: { ...nativeActor, enabled: true, running: true } });
  expect(host.querySelector("details")?.open).toBe(false);
  expect(host.textContent).toContain(enActors.nativeSession.recovery.freshnessHint);
  await expand(true);
  expect(host.querySelector("details")?.open).toBe(true);
  await expand(false);
  await expand(true);
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(mocks.refresh).not.toHaveBeenCalled();
  await render({ actor: { ...nativeActor, generation: "offline-generation" } });
  expect(host.querySelector("details")?.open).toBe(true);
});

it("reads again on Actor running/enabled and current Group state/running changes, not unrelated snapshots", async () => {
  useGroupStore.setState({ groupDoc: { group_id: GROUP_ID, state: "active", running: true } });
  await render();
  await render({ actor: { ...nativeActor, enabled: true } });
  await render({ actor: { ...nativeActor, enabled: true, running: true } });
  await act(async () =>
    useGroupStore.setState({ groupDoc: { group_id: GROUP_ID, state: "paused", running: true } }),
  );
  await act(async () =>
    useGroupStore.setState({ groupDoc: { group_id: GROUP_ID, state: "paused", running: false } }),
  );
  expect(mocks.load).toHaveBeenCalledTimes(5);
  await render({ actor: { ...nativeActor, enabled: true, running: true, title: "New label" } });
  await act(async () =>
    useGroupStore.setState({
      groupDoc: { group_id: GROUP_ID, state: "paused", running: false, title: "New Group label" },
    }),
  );
  await advance(60000);
  expect(mocks.load).toHaveBeenCalledTimes(5);
  expect(mocks.refresh).not.toHaveBeenCalled();
});

it("uses the one refresh action for End/disable state and keeps Claude refresh passive", async () => {
  await render();
  for (const state of ["session_ended", "integration_disabled"] as const) {
    mocks.refresh.mockResolvedValueOnce(recoveryResponse({ state }));
    await clickButton("Refresh status");
    expect(host.textContent).toContain(enActors.nativeSession.recovery.states[state]);
  }
  expect(mocks.load).toHaveBeenCalledOnce();
  const actor = { ...nativeActor, runtime: "claude", external_session_id: "claude:host" };
  mocks.load.mockResolvedValue(recoveryResponse({}, actor));
  await render({ actor });
  await clickButton("Refresh status");
  expect(mocks.load).toHaveBeenCalledTimes(3);
  expect(mocks.refresh).toHaveBeenCalledTimes(2);
});

it("keeps the validated snapshot when qualification is inconclusive, without a second GET", async () => {
  const confirmed = { command: "authorized recovery command", cwd: "/synthetic/confirmed-source" };
  const response = recoveryResponse(confirmed);
  response.result.refresh_status = "unavailable";
  mocks.refresh.mockResolvedValueOnce(response);
  await render();
  await clickButton("Refresh status");
  expect(host.querySelector("pre code")?.textContent).toBe(confirmed.command);
  expect(host.textContent).toContain(confirmed.cwd);
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(
    enActors.nativeSession.recovery.refreshFailed,
  );
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(mocks.refresh).toHaveBeenCalledOnce();
});

it("keeps one refresh in flight and allows an explicit retry after an inconclusive result", async () => {
  const refresh = deferred<ReturnType<typeof recoveryResponse>>();
  mocks.refresh.mockReturnValueOnce(refresh.promise);
  await render();
  expect(
    [...host.querySelectorAll("button")].filter((b) => b.textContent === "Refresh status"),
  ).toHaveLength(1);
  await clickButton("Refresh status");
  const button = [...host.querySelectorAll("button")].find(
    (b) => b.textContent === "Refresh status",
  )!;
  expect(button.disabled).toBe(true);
  expect(host.textContent).toContain(enActors.nativeSession.recovery.refreshingConnection);
  await act(async () => button.click());
  await advance(60000);
  expect(mocks.refresh).toHaveBeenCalledExactlyOnceWith(GROUP_ID, nativeActor.id);
  expect(mocks.load).toHaveBeenCalledOnce();
  const confirmed = recoveryResponse({ command: "confirmed command", cwd: "/synthetic/confirmed" });
  confirmed.result.refresh_status = "unavailable";
  await act(async () => refresh.resolve(confirmed));
  expect(button.disabled).toBe(false);
  expect(host.querySelector("pre code")?.textContent).toBe(confirmed.result.recovery.command);
  expect(host.textContent).not.toContain(originalCommand);
  await clickButton("Copy command");
  expect(mocks.clipboard).toHaveBeenLastCalledWith(confirmed.result.recovery.command);
  await advance(60000);
  expect(mocks.refresh).toHaveBeenCalledOnce();
  expect(mocks.load).toHaveBeenCalledOnce();
  await clickButton("Refresh status");
  expect(mocks.refresh).toHaveBeenCalledTimes(2);
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(host.textContent).toContain(enActors.nativeSession.recovery.refreshConnected);
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(nativeActor.enabled).toBe(false);
});

it.each([
  { name: "backend source replacement before props update", binding: { source_id: "codex:new" } },
  { name: "wrong Group", binding: { group_id: "g_other" } },
  { name: "wrong Actor", binding: { actor_id: "other" } },
  { name: "permission denied", error: "permission_denied" },
  { name: "read failure", error: "read_failed" },
  { name: "rejected read", rejected: true },
])("clears private details after $name during refresh", async (failure) => {
  if (failure.rejected) mocks.refresh.mockRejectedValueOnce(new Error("Synthetic read failure"));
  else if (failure.error)
    mocks.refresh.mockResolvedValueOnce({
      ok: false,
      error: { code: failure.error, message: "Synthetic denied/failed read" },
    });
  else {
    const response = recoveryResponse({
      command: "wrong-binding-command",
      cwd: "/synthetic/other",
    });
    response.result.refresh_status = "unavailable";
    mocks.refresh.mockResolvedValueOnce({
      ...response,
      result: { ...response.result, ...failure.binding },
    });
  }
  await render();
  await clickButton("Refresh status");
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(host.querySelector("pre")).toBeNull();
  expect(host.textContent).not.toContain(originalCwd);
  expect(host.textContent).not.toContain("wrong-binding-command");
  expect(host.textContent).not.toContain("/synthetic/other");
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(
    enActors.nativeSession.recovery.refreshFailed,
  );
  await advance(60000);
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(mocks.refresh).toHaveBeenCalledOnce();
  await render({ actor: { ...nativeActor, enabled: true } });
  expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
  expect(mocks.refresh).toHaveBeenCalledOnce();
});

it.each(["permission_denied", "refresh_failed"])(
  "does not revalidate or retain a snapshot after other POST errors (%s)",
  async (code) => {
    mocks.refresh.mockResolvedValueOnce({ ok: false, error: { code, message: "Synthetic error" } });
    await render();
    await clickButton("Refresh status");
    expect(host.querySelector("pre")).toBeNull();
    expect(host.textContent).not.toContain(originalCwd);
    expect(mocks.load).toHaveBeenCalledOnce();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      enActors.nativeSession.recovery.refreshFailed,
    );
  },
);

it.each([
  { name: "Group", patch: { groupId: "g_changed" } },
  { name: "Actor", patch: { actor: { ...nativeActor, id: "replacement" } } },
  { name: "source", patch: { actor: { ...nativeActor, external_session_id: "codex:changed" } } },
  { name: "generation", patch: { actor: { ...nativeActor, generation: "changed" } } },
  { name: "permission", patch: { canAccessRecovery: false } },
  { name: "visibility", patch: { isVisible: false } },
  { name: "unmount", patch: null },
])("fences a late refresh snapshot across $name changes", async ({ patch }) => {
  const revalidation = deferred<ReturnType<typeof recoveryResponse>>();
  mocks.refresh.mockReturnValueOnce(revalidation.promise);
  await render();
  await clickButton("Refresh status");
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(host.querySelector('section[aria-busy="true"]')).not.toBeNull();
  if (patch) {
    mocks.load.mockResolvedValueOnce(
      recoveryResponse(
        { command: "current-binding-command", cwd: "/synthetic/current" },
        patch.actor ?? nativeActor,
        patch.groupId ?? GROUP_ID,
      ),
    );
    await render(patch);
  } else await act(async () => root.render(null));
  await act(async () =>
    revalidation.resolve(
      recoveryResponse({ command: "late-revalidation-command", cwd: "/synthetic/late" }),
    ),
  );
  expect(host.textContent).not.toContain("late-revalidation-command");
  expect(host.textContent).not.toContain("/synthetic/late");
  expect(host.textContent).not.toContain(originalCwd);
  if (patch && patch.canAccessRecovery !== false && patch.isVisible !== false)
    expect(host.querySelector("pre code")?.textContent).toBe("current-binding-command");
  else expect(host.querySelector("section")).toBeNull();
  await advance(60000);
  expect(mocks.refresh).toHaveBeenCalledOnce();
});

it.each(["before", "after"])(
  "keeps explicit refresh feedback when runtime synchronization arrives %s its response",
  async (order) => {
    const refresh = deferred<ReturnType<typeof recoveryResponse>>();
    mocks.refresh.mockReturnValueOnce(refresh.promise);
    const offline = { ...nativeActor, enabled: true, running: false };
    const online = { ...offline, running: true };
    await render({ actor: offline });
    mocks.load.mockResolvedValue(recoveryResponse({ state: "connected" }));
    await clickButton("Refresh status");
    if (order === "before") await render({ actor: online });
    await act(async () => refresh.resolve(recoveryResponse({ state: "connected" })));
    if (order === "after") await render({ actor: online });
    expect(host.textContent).toContain(enActors.nativeSession.recovery.refreshConnected);
    expect(host.querySelector('section[aria-busy="true"]')).toBeNull();
    expect(mocks.refresh).toHaveBeenCalledOnce();

    mocks.load.mockResolvedValue(recoveryResponse());
    await render({ actor: offline });
    expect(host.textContent).toContain(enActors.nativeSession.recovery.states.disconnected);
    expect(host.textContent).not.toContain(enActors.nativeSession.recovery.refreshConnected);
  },
);

it("reports a negative refresh without re-enabling access or retrying automatically", async () => {
  mocks.refresh.mockResolvedValue(recoveryResponse());
  await render();
  await clickButton("Refresh status");
  expect(host.textContent).toContain(enActors.nativeSession.recovery.refreshUnavailable);
  expect(host.querySelector('[role="status"]')?.textContent).toBe("CCCC access paused");
  await advance(60000);
  expect(mocks.refresh).toHaveBeenCalledOnce();
  expect(mocks.load).toHaveBeenCalledOnce();
});

it.each(["new source", "lost permission"])(
  "ignores a late explicit refresh after %s",
  async (change) => {
    const refresh = deferred<ReturnType<typeof recoveryResponse>>();
    mocks.refresh.mockReturnValueOnce(refresh.promise);
    await render();
    await clickButton("Refresh status");
    if (change === "new source") {
      const actor = {
        ...nativeActor,
        external_session_id: "codex:replacement",
        generation: "replacement",
      };
      mocks.load.mockResolvedValueOnce(recoveryResponse({ command: "new-source-command" }, actor));
      await render({ actor });
    } else await render({ canAccessRecovery: false });
    await act(async () =>
      refresh.resolve(recoveryResponse({ state: "connected", command: "late-refresh-command" })),
    );
    expect(host.textContent).not.toContain("late-refresh-command");
    expect(host.textContent).not.toContain(enActors.nativeSession.recovery.refreshConnected);
    if (change === "new source")
      expect(host.querySelector("pre code")?.textContent).toBe("new-source-command");
    else expect(host.querySelector("section")).toBeNull();
  },
);

it.each([
  { name: "modern clipboard success", modern: true, fallback: false, success: true },
  { name: "fallback success", modern: false, fallback: true, success: true },
  { name: "both clipboard methods failing", modern: false, fallback: false, success: false },
])(
  "reports $name inline for both fields even while Actor access is paused",
  async ({ modern, fallback, success }) => {
    if (!modern) mocks.clipboard.mockRejectedValue(new Error("Synthetic clipboard denial"));
    mocks.fallback.mockReturnValue(fallback);
    await render();
    for (const [label, value] of [
      ["Copy command", originalCommand],
      ["Copy directory", originalCwd],
    ]) {
      await clickButton(label);
      expect(mocks.clipboard).toHaveBeenLastCalledWith(value);
      if (success) {
        expect(host.textContent).toContain("Copied");
        expect(host.querySelector('[role="alert"]')).toBeNull();
      } else {
        expect(host.querySelector('[role="alert"]')?.textContent).toBe(
          enActors.nativeSession.recovery.copyFailed,
        );
        expect(host.textContent).not.toContain("Copied");
      }
      expect(host.querySelector("textarea")).toBeNull();
    }
    expect(mocks.fallback).toHaveBeenCalledTimes(modern ? 0 : 2);
    expect(host.querySelector("pre code")?.textContent).toBe(originalCommand);
    expect(mocks.load).toHaveBeenCalledOnce();
  },
);

it("clears an inline copy failure after a later successful copy", async () => {
  mocks.clipboard.mockRejectedValueOnce(new Error("Synthetic clipboard denial"));
  await render();
  await clickButton("Copy command");
  expect(host.querySelector('[role="alert"]')).not.toBeNull();
  await clickButton("Copy command");
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(host.textContent).toContain("Copied");
});

it.each(["binding", "permission"])(
  "ignores late copy feedback after a %s change",
  async (change) => {
    const pendingCopy = deferred<void>();
    mocks.clipboard.mockReturnValueOnce(pendingCopy.promise);
    await render();
    await clickButton("Copy command");
    if (change === "binding") {
      const actor = { ...nativeActor, external_session_id: "codex:new-thread" };
      mocks.load.mockResolvedValueOnce(recoveryResponse({}, actor));
      await render({ actor });
    } else {
      await render({ canAccessRecovery: false });
      await render();
    }
    await act(async () => pendingCopy.resolve());
    expect(host.textContent).not.toContain("Copied");
    expect(host.querySelector('[role="alert"]')).toBeNull();
  },
);

it("reads the Actor list once after a confirmed Codex refresh, never after reads or failures", async () => {
  await render();
  expect(mocks.refreshActors).not.toHaveBeenCalled();
  await clickButton("Refresh status");
  expect(mocks.refreshActors).toHaveBeenCalledExactlyOnceWith(GROUP_ID);

  mocks.refresh.mockResolvedValueOnce({
    ok: false,
    error: { code: "refresh_failed", message: "Synthetic error" },
  });
  await clickButton("Refresh status");
  mocks.refresh.mockResolvedValueOnce(
    recoveryResponse({ state: "connected" }, nativeActor, "g_wrong"),
  );
  await clickButton("Refresh status");
  expect(mocks.refreshActors).toHaveBeenCalledOnce();

  // Claude's Refresh is the passive read, which changes nothing to re-read.
  await render({
    actor: { ...nativeActor, runtime: "claude", external_session_id: "claude:host" },
  });
  mocks.load.mockResolvedValue(
    recoveryResponse({}, { ...nativeActor, runtime: "claude", external_session_id: "claude:host" }),
  );
  await clickButton("Refresh status");
  expect(mocks.refreshActors).toHaveBeenCalledOnce();
});
