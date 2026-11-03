// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { HeadlessPreviewSession } from "../../types";
import { HeadlessLiveTrace } from "./HeadlessLiveTrace";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
    i18n: { language: "en" },
  }),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("shows nonredundant tool summaries in the compact trace", async () => {
  const longTool = `tool-${"x".repeat(120)}`;
  const session: HeadlessPreviewSession = {
    actorId: "worker",
    pendingEventId: "pending-1",
    currentStreamId: "stream-1",
    phase: "streaming",
    streamPhase: "commentary",
    updatedAt: "2026-11-01T00:00:00Z",
    latestText: "",
    transcriptBlocks: [],
    activities: [
      {
        id: "failure",
        kind: "tool",
        status: "completed",
        command: "cargo test",
        tool_name: "bash",
        summary: "bash failed",
      },
      {
        id: "success",
        kind: "tool",
        status: "completed",
        command: "git status",
        tool_name: "bash",
        summary: "bash",
      },
      { id: "progress", kind: "thinking", status: "updated", summary: "Checking the workspace" },
      {
        id: "long",
        kind: "tool",
        status: "completed",
        command: "x".repeat(120),
        tool_name: longTool,
        summary: `${longTool} failed`,
      },
    ],
  };
  await act(async () => {
    root.render(
      <HeadlessLiveTrace
        previewSessions={[session]}
        emptyLabel="Empty"
        isDark={false}
        density="compact"
      />,
    );
  });

  const row = (title: string) =>
    [...host.querySelectorAll<HTMLElement>("[title]")].find(
      (element) => element.getAttribute("title") === title,
    );
  expect(row("cargo test\nbash failed")?.textContent).toContain("bash failed");
  expect(row("git status\nbash")?.textContent).not.toContain("bash");
  expect(host.textContent?.match(/Checking the workspace/g)).toHaveLength(1);
  expect(host.textContent).toContain(`${longTool} failed`);
});
