// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, afterEach, it, expect, vi } from "vite-plus/test";
const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  cancel: vi.fn(),
  reset: vi.fn(),
  terminal: vi.fn(),
  t: (key: string) => key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("../../../services/api", () => ({
  fetchSecretaryRuntime: mocks.fetch,
  cancelSecretaryTask: mocks.cancel,
  resetSecretaryRuntime: mocks.reset,
  getSecretaryTerminalWebSocketUrl: () => "ws://fixture/term",
}));
vi.mock("../../../features/voice/NativeSessionTerminal", () => ({
  NativeSessionTerminal: (props: { generation: string; readOnly: boolean }) => {
    mocks.terminal(props);
    return <div data-terminal={props.generation} />;
  },
}));
import { useUIStore } from "../../../stores/useUIStore";
import { SecretaryRuntimePanel } from "./SecretaryRuntimePanel";
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const ready = { phase: "ready", generation: "session-1", runtime: "codex", native_terminal: true };
const response = (result: object) => ({ ok: true, result });
const render = (active = true) =>
  act(async () => root.render(<SecretaryRuntimePanel active={active} isDark={false} />));
const button = (key: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === key)!;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  useUIStore.setState({ canAccessGlobalSettings: true });
  vi.useFakeTimers();
  for (const mock of [mocks.fetch, mocks.cancel, mocks.reset, mocks.terminal]) mock.mockReset();
  mocks.fetch.mockResolvedValue(response(ready));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
it("keeps an idle native terminal visible without starting or resetting any work", async () => {
  await render();
  expect(host.querySelector('[data-terminal="session-1"]')).not.toBeNull();
  expect(mocks.terminal.mock.lastCall?.[0].readOnly).toBeFalsy();
  await act(async () => vi.advanceTimersByTimeAsync(4000));
  expect(mocks.fetch).toHaveBeenCalledTimes(3);
  expect(mocks.reset).not.toHaveBeenCalled();
  expect(mocks.cancel).not.toHaveBeenCalled();
  mocks.reset.mockResolvedValue(response({ resetting: true }));
  mocks.fetch.mockResolvedValue(response({ phase: "not_started" }));
  await act(async () => button("voiceSettings.resident.newSession").click());
  expect(mocks.reset).toHaveBeenCalledWith("session-1");
  expect(host.querySelector("[data-terminal]")).toBeNull();
});
it("identifies the actual Group and cancels that fixed task on the ACP surface", async () => {
  mocks.fetch.mockResolvedValue(
    response({
      phase: "working",
      generation: "ACP",
      runtime: "copilot",
      native_terminal: false,
      group_title: "Group B",
      task: {
        task_id: "B-task",
        preview: "Check the budget",
        target: { group_id: "B", kind: "ask" },
      },
      progress: "Checking source data",
    }),
  );
  await render();
  expect(host.textContent).toContain("Group B");
  expect(host.textContent).toContain("Check the budget");
  expect(host.textContent).toContain("Checking source data");
  expect(mocks.terminal).not.toHaveBeenCalled();
  mocks.cancel.mockResolvedValue(response({ cancel_requested: true }));
  await act(async () => button("voiceSettings.cancelTask").click());
  expect(mocks.cancel).toHaveBeenCalledWith("B", "B-task");
});
it("shows a manual terminal turn without borrowing the previous task or allowing a reset", async () => {
  mocks.fetch.mockResolvedValue(response({ ...ready, phase: "working", manual_turn: true }));
  await render();
  expect(host.textContent).toContain("voiceSettings.resident.manualTurn");
  expect(button("voiceSettings.resident.newSession").disabled).toBe(true);
  expect(button("voiceSettings.cancelTask")).toBeUndefined();
  expect(host.querySelector("[data-terminal]")).not.toBeNull();
});
it("rejects a slow older snapshot and pauses observations when hidden", async () => {
  let old!: (value: unknown) => void;
  mocks.fetch.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        old = resolve;
      }),
  );
  await render();
  await act(async () => vi.advanceTimersByTime(2000));
  expect(mocks.fetch).toHaveBeenCalledTimes(1);
  await render(false);
  await render();
  expect(host.textContent).toContain("voiceSettings.resident.phases.ready");
  await act(async () =>
    old(response({ phase: "working", runtime: "copilot", progress: "obsolete" })),
  );
  expect(host.textContent).not.toContain("obsolete");
  await render(false);
  const count = mocks.fetch.mock.calls.length;
  await act(async () => vi.advanceTimersByTime(6000));
  expect(mocks.fetch).toHaveBeenCalledTimes(count);
  expect(mocks.reset).not.toHaveBeenCalled();
});

it("publishes slow runtime reads without overlapping or discarding every response", async () => {
  mocks.fetch.mockImplementation(
    () => new Promise((resolve) => setTimeout(() => resolve(response(ready)), 2500)),
  );
  await render();
  await act(async () => vi.advanceTimersByTimeAsync(2500));
  expect(host.textContent).toContain("voiceSettings.resident.phases.ready");
  expect(mocks.fetch).toHaveBeenCalledTimes(1);
  await act(async () => vi.advanceTimersByTimeAsync(4000));
  expect(mocks.fetch).toHaveBeenCalledTimes(2);
  expect(host.querySelector('[data-terminal="session-1"]')).not.toBeNull();
  expect(mocks.reset).not.toHaveBeenCalled();
});

it("does not expose the shared terminal or poll after admin access is revoked", async () => {
  await render();
  expect(host.querySelector("[data-terminal]")).not.toBeNull();
  await act(async () => useUIStore.setState({ canAccessGlobalSettings: false }));
  const count = mocks.fetch.mock.calls.length;
  await act(async () => vi.advanceTimersByTime(6000));
  expect(mocks.fetch).toHaveBeenCalledTimes(count);
  expect(host.querySelector("[data-terminal]")).toBeNull();
  expect(host.textContent).toContain("voiceSettings.resident.adminOnly");
});
