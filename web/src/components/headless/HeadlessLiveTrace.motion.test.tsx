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
vi.mock("../LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: ({ content }: { content: string }) => (
    <div className="markdown-body">
      <p>
        {content === "A [link](https://example.invalid)" ? (
          <a href="https://example.invalid">link</a>
        ) : (
          content
        )}
      </p>
    </div>
  ),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: Root;

function session(
  text: string,
  options: { phase?: string; streamPhase?: string; pendingId?: string; completed?: boolean } = {},
): HeadlessPreviewSession {
  return {
    actorId: "worker",
    pendingEventId: options.pendingId || "pending-1",
    currentStreamId: "stream-1",
    phase: options.phase || "streaming",
    streamPhase: options.streamPhase || "commentary",
    updatedAt: "2026-10-31T00:00:00Z",
    latestText: text,
    transcriptBlocks: [
      {
        id: "block-1",
        streamId: "stream-1",
        streamPhase: options.streamPhase || "commentary",
        text,
        updatedAt: "2026-10-31T00:00:00Z",
        completed: options.completed || false,
        transient: true,
      },
    ],
    activities: [],
  };
}

async function render(sessions: HeadlessPreviewSession[]) {
  await act(async () =>
    root.render(<HeadlessLiveTrace previewSessions={sessions} emptyLabel="Empty" isDark={false} />),
  );
}

beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("treats initial text as a baseline, then responds only to real text changes", async () => {
  await render([session("First")]);
  const cursor = () => host.querySelector<HTMLElement>("span[data-lcd-live-cursor]");
  expect(cursor()?.dataset.lcdLiveCursor).toBe("waiting");
  expect(cursor()?.getAttribute("aria-hidden")).toBe("true");
  await render([session("First")]);
  expect(cursor()?.dataset.lcdLiveCursor).toBe("waiting");
  await render([session("First chunk")]);
  expect(cursor()?.dataset.lcdLiveCursor).toBe("solid");
  await act(async () => vi.advanceTimersByTime(700));
  expect(cursor()?.dataset.lcdLiveCursor).toBe("waiting");
  await act(async () => vi.advanceTimersByTime(9_300));
  expect(cursor()?.dataset.lcdLiveCursor).toBe("quiet");
  await render([session("First chunk plus another")]);
  expect(cursor()?.dataset.lcdLiveCursor).toBe("solid");
  await render([session("First chunk plus another", { phase: "completed", completed: true })]);
  expect(cursor()).toBeNull();
});

it("does not imply that pending, completed or unverified fallback text is streaming", async () => {
  await render([session("Saved", { phase: "pending" })]);
  expect(host.querySelector("[data-lcd-live-cursor]")).toBeNull();
  await render([session("Saved", { phase: "completed", completed: true })]);
  expect(host.querySelector("[data-lcd-live-cursor]")).toBeNull();
  await act(async () =>
    root.render(<HeadlessLiveTrace fallbackText="Saved" emptyLabel="Empty" isDark={false} />),
  );
  expect(host.querySelector("[data-lcd-live-cursor]")).toBeNull();
});

it("does not reset the quiet timer when activity changes but the text does not", async () => {
  const first = session("First");
  await render([first]);
  await act(async () => vi.advanceTimersByTime(5_000));
  await render([
    {
      ...first,
      activities: [
        {
          id: "tool-1",
          kind: "tool",
          status: "updated",
          summary: "A tool is running",
          ts: "2026-10-31T00:00:01Z",
        },
      ],
    },
  ]);
  await act(async () => vi.advanceTimersByTime(5_000));
  expect(host.querySelector<HTMLElement>("[data-lcd-live-cursor]")?.dataset.lcdLiveCursor).toBe(
    "quiet",
  );
});

it("does not mark restored history as a new chunk when the session changes", async () => {
  await render([session("One")]);
  await render([session("One changed")]);
  expect(host.querySelector<HTMLElement>("[data-lcd-live-cursor]")?.dataset.lcdLiveCursor).toBe(
    "solid",
  );
  await render([session("Existing text", { pendingId: "pending-2" })]);
  expect(host.querySelector<HTMLElement>("[data-lcd-live-cursor]")?.dataset.lcdLiveCursor).toBe(
    "waiting",
  );
});

it("marks Markdown as a container without inserting the cursor into rendered content", async () => {
  await render([session("A **message**", { streamPhase: "final_answer" })]);
  const wrapper = host.querySelector<HTMLElement>("[data-lcd-live-markdown]");
  expect(wrapper?.dataset.lcdLiveCursor).toBe("waiting");
  expect(wrapper?.querySelector(".markdown-body")?.textContent).toBe("A **message**");
  expect(wrapper?.querySelector(".markdown-body [data-lcd-live-cursor]")).toBeNull();
});

it("keeps focus in Markdown when the live cursor disappears", async () => {
  const text = "A [link](https://example.invalid)";
  await render([session(text, { streamPhase: "final_answer" })]);
  const link = host.querySelector<HTMLAnchorElement>(".markdown-body a")!;
  link.focus();
  const selection = window.getSelection()!;
  const range = document.createRange();
  range.selectNodeContents(link);
  selection.removeAllRanges();
  selection.addRange(range);
  expect(document.activeElement).toBe(link);
  expect(selection.toString()).toBe("link");

  await render([
    session(text, { phase: "completed", streamPhase: "final_answer", completed: true }),
  ]);
  expect(host.querySelector(".markdown-body a")).toBe(link);
  expect(document.activeElement).toBe(link);
  expect(selection.toString()).toBe("link");
  expect(host.querySelector("[data-lcd-live-cursor]")).toBeNull();

  await render([session(text, { streamPhase: "final_answer" })]);
  expect(host.querySelector(".markdown-body a")).toBe(link);
  expect(document.activeElement).toBe(link);
  expect(host.querySelector("[data-lcd-live-cursor]")).not.toBeNull();
});
