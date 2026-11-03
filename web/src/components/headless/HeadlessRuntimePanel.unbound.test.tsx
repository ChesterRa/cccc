// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { HeadlessPreviewSession, HeadlessStreamEvent } from "../../types";
import { HeadlessRuntimePanel } from "./HeadlessRuntimePanel";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) => opts?.defaultValue || key,
    i18n: { language: "en" },
  }),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("keeps an older requested reply and a newer text-bearing turn's tool visible together", async () => {
  const previous: HeadlessPreviewSession = {
    actorId: "a",
    pendingEventId: "real-request",
    currentStreamId: "old",
    phase: "completed",
    streamPhase: "",
    updatedAt: "2026-01-01T00:00:01Z",
    latestText: "Earlier reply",
    activities: [],
    transcriptBlocks: [
      {
        id: "old",
        streamId: "old",
        streamPhase: "",
        text: "Earlier reply",
        updatedAt: "2026-01-01T00:00:01Z",
        completed: true,
        transient: false,
      },
    ],
  };
  const ev: HeadlessStreamEvent = {
    id: "a1",
    group_id: "g",
    actor_id: "a",
    type: "headless.activity.completed",
    ts: "2026-01-01T00:00:02Z",
    data: {
      stream_id: "new",
      activity_id: "tool",
      kind: "tool",
      summary: "bash failed",
      tool_name: "bash",
      command: "cargo test",
    },
  };
  const rows = [
    {
      id: "stream:new",
      group_id: "g",
      by: "a",
      kind: "chat.message",
      ts: "2026-01-01T00:00:03Z",
      _streaming: true,
      data: {
        stream_id: "new",
        text: "New reply",
        activities: [
          {
            id: "tool",
            kind: "tool",
            status: "completed",
            summary: "bash failed",
            command: "cargo test",
            ts: "2026-01-01T00:00:02Z",
          },
        ],
        _headless_text_started_at: "2026-01-01T00:00:03Z",
      },
    },
  ];
  const host = document.createElement("div"),
    root = createRoot(host);
  document.body.append(host);
  try {
    await act(async () =>
      root.render(
        <HeadlessRuntimePanel
          groupId="g"
          actorId="a"
          previewSessions={[previous]}
          streamingEvents={rows}
          fallbackText="Earlier reply"
          fallbackActivities={[]}
          rawEvents={[ev]}
          emptyLabel="Empty"
          isDark={false}
          compact
        />,
      ),
    );
    expect(host.textContent).toContain("Earlier reply");
    expect(host.textContent).toContain("New reply");
    expect(host.textContent).toContain("bash failed");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
