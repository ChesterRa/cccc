// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceAnalystInfo } from "../../services/api/codexVoice";
import { controlCodexVoiceAnalyst, fetchActiveCodexVoiceCall } from "../../services/api/codexVoice";
import { VoiceAcpConsole } from "./VoiceAcpConsole";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api/codexVoice", () => ({
  fetchActiveCodexVoiceCall: vi.fn(async () => ({ ok: false })),
  controlCodexVoiceAnalyst: vi.fn(),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => {
  vi.mocked(fetchActiveCodexVoiceCall)
    .mockReset()
    .mockResolvedValue({ ok: false } as never);
  vi.mocked(controlCodexVoiceAnalyst).mockReset();
});
afterEach(() => {
  document.body.innerHTML = "";
  vi.useRealTimers();
});

async function renderConsole(analyst: CodexVoiceAnalystInfo) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <VoiceAcpConsole
        analyst={analyst}
        visible={false}
        call={null}
        onAnalystSnapshot={() => {}}
      />,
    ),
  );
  await vi.waitFor(() => expect(host.querySelector("strong")).not.toBeNull());
  return { host, root };
}

const baseAnalyst: CodexVoiceAnalystInfo = {
  generation: "acp-analyst",
  structured: true,
  tui_ready: false,
  phase: "ready",
  last_result: "",
  warning: "",
  queued_inputs: 0,
  manual_tasks: [],
};

it("renders the latest turn result as Markdown instead of raw source", async () => {
  const { host, root } = await renderConsole({
    ...baseAnalyst,
    last_result: "**Build is green**\n\n- 3 tests fixed",
  });
  expect(host.querySelector("strong")?.textContent).toBe("Build is green");
  expect(host.querySelector("li")?.textContent).toBe("3 tests fixed");
  expect(host.textContent).not.toContain("**");
  await act(async () => root.unmount());
});

it("renders manual investigation results as Markdown", async () => {
  const { host, root } = await renderConsole({
    ...baseAnalyst,
    manual_task_id: "task-1",
    manual_tasks: [
      {
        id: "task-1",
        text: "Why did CI fail?",
        call_generation: null,
        status: "completed",
        result: "**Flaky test** in `voice_ops`",
        error: "",
      },
    ],
  });
  expect(host.querySelector("strong")?.textContent).toBe("Flaky test");
  expect(host.querySelector("code")?.textContent).toBe("voice_ops");
  expect(host.textContent).not.toContain("**");
  await act(async () => root.unmount());
});

it("keeps its snapshot through transient reads, warns on sustained failure and clears on recovery", async () => {
  vi.useFakeTimers();
  const analyst = { ...baseAnalyst, last_result: "Previous answer" };
  const snapshot = vi.fn();
  const fetch = vi.mocked(fetchActiveCodexVoiceCall);
  fetch
    .mockResolvedValueOnce({ ok: false } as never)
    .mockRejectedValueOnce(new Error("Temporary fixture read failure"))
    .mockResolvedValueOnce({ ok: true, result: { analyst } } as never);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <VoiceAcpConsole analyst={analyst} visible call={null} onAnalystSnapshot={snapshot} />,
    ),
  );
  expect(host.textContent).toContain("Previous answer");
  expect(host.querySelector("[data-acp-snapshot-stale]")).toBeNull();
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.querySelector("[data-acp-snapshot-stale]")?.textContent).toBe(
    "acpControls.snapshotStale",
  );
  expect(host.textContent).toContain("Previous answer");
  expect(snapshot).not.toHaveBeenCalled();
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.querySelector("[data-acp-snapshot-stale]")).toBeNull();
  expect(snapshot).toHaveBeenLastCalledWith(analyst);
  await act(async () => root.unmount());
});

it("does not carry failed reads from a replaced Analyst into the new snapshot", async () => {
  vi.useFakeTimers();
  const oldAnalyst = { ...baseAnalyst, last_result: "Previous answer" };
  const nextAnalyst = { ...oldAnalyst, generation: "replacement", last_result: "Current answer" };
  const snapshot = vi.fn();
  let failOld!: (error: Error) => void;
  const fetch = vi.mocked(fetchActiveCodexVoiceCall);
  fetch
    .mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          failOld = reject;
        }),
    )
    .mockResolvedValueOnce({ ok: true, result: { analyst: nextAnalyst } } as never)
    .mockResolvedValueOnce({ ok: false } as never);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (analyst: CodexVoiceAnalystInfo) =>
    act(async () =>
      root.render(
        <VoiceAcpConsole analyst={analyst} visible call={null} onAnalystSnapshot={snapshot} />,
      ),
    );
  await render(oldAnalyst);
  await render(nextAnalyst);
  await act(async () => failOld(new Error("Old fixture read failure")));
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.querySelector("[data-acp-snapshot-stale]")).toBeNull();
  expect(host.textContent).toContain("Current answer");
  expect(snapshot).toHaveBeenCalledTimes(1);
  expect(fetch).toHaveBeenCalledTimes(3);
  await act(async () => root.unmount());
});

it("clears the stale hint after an authoritative control reply and starts a new failure sequence", async () => {
  vi.useFakeTimers();
  const analyst = {
    ...baseAnalyst,
    permissions: [{ request_id: "fixture-request", title: "Fixture approval", kind: "execute" }],
  };
  const nextAnalyst = { ...analyst, permissions: [] };
  const snapshot = vi.fn();
  vi.mocked(controlCodexVoiceAnalyst).mockResolvedValueOnce({
    ok: true,
    result: { analyst: nextAnalyst },
  } as never);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <VoiceAcpConsole analyst={analyst} visible call={null} onAnalystSnapshot={snapshot} />,
    ),
  );
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.querySelector("[data-acp-snapshot-stale]")).not.toBeNull();
  const deny = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "acpControls.deny",
  )!;
  await act(async () => deny.click());
  expect(host.querySelector("[data-acp-snapshot-stale]")).toBeNull();
  expect(snapshot).toHaveBeenLastCalledWith(nextAnalyst, true);
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.querySelector("[data-acp-snapshot-stale]")).toBeNull();
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.querySelector("[data-acp-snapshot-stale]")).not.toBeNull();
  await act(async () => root.unmount());
});
