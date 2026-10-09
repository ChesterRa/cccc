// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import type { CodexVoiceAnalystInfo } from "../../services/api/codexVoice";
import { VoiceAcpConsole } from "./VoiceAcpConsole";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api/codexVoice", () => ({
  fetchActiveCodexVoiceCall: vi.fn(async () => ({ ok: false })),
  controlCodexVoiceAnalyst: vi.fn(),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => {
  document.body.innerHTML = "";
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
