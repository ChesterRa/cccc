// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useChatOutboxStore } from "../stores/chatOutboxStore";
import { useResourcePanelStore } from "../stores/useResourcePanelStore";
import type { LedgerEvent } from "../types";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "../utils/chatOutboxReconciliation";
import { MessageBubble } from "./MessageBubble";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  useChatOutboxStore.getState().clearAll();
  useResourcePanelStore.setState({ groups: {} });
  vi.useRealTimers();
  vi.restoreAllMocks();
});

async function renderMessage(event: LedgerEvent) {
  await act(async () => {
    root.render(
      <MessageBubble
        event={event}
        actorById={new Map()}
        actors={[]}
        displayNameMap={new Map()}
        agentState={null}
        taskById={new Map()}
        isDark={false}
        groupId="displayed-group"
        groupLabelById={{}}
        onReply={() => undefined}
        onShowRecipients={() => undefined}
      />,
    );
  });
}

it("keeps original attachment identity through filtering, reordering and send confirmation", async () => {
  vi.useFakeTimers();
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  const optimistic = {
    id: "client-1",
    ts: "2026-11-08T06:00:00Z",
    kind: "chat.message",
    by: "user",
    data: {
      client_id: "client-1",
      _optimistic: true,
      attachments: [
        { path: "unavailable/ignored" },
        { path: "", title: "image.png", mime_type: "image/png", local_preview_url: "blob:image" },
        null,
        { path: "", title: "video.mp4", mime_type: "video/mp4", local_preview_url: "blob:video" },
        {
          path: "",
          title: "notes.md",
          mime_type: "text/markdown",
          local_preview_url: "blob:notes",
        },
      ],
    },
  } as unknown as LedgerEvent;
  const confirmed = {
    id: "event-1",
    ts: "2026-11-08T06:00:01Z",
    group_id: "blob-owner",
    kind: "chat.message",
    by: "user",
    data: {
      client_id: "client-1",
      attachments: [
        { path: "unavailable/ignored" },
        { path: "state/blobs/image", title: "image.png", mime_type: "image/png" },
        null,
        { path: "state/blobs/video", title: "video.mp4", mime_type: "video/mp4" },
        { path: "state/blobs/notes", title: "notes.md", mime_type: "text/markdown" },
      ],
    },
  } as unknown as LedgerEvent;
  useChatOutboxStore.getState().enqueue("displayed-group", "client-1", optimistic);
  await renderMessage(optimistic);
  const click = (selector: string) => {
    const button = host.querySelector<HTMLButtonElement>(selector);
    expect(button).not.toBeNull();
    act(() => button!.click());
  };
  click("[data-image-frame]");
  click('button[aria-label="playVideo"]');
  click('button[aria-label="previewFile"]');
  const pending = useResourcePanelStore.getState().groups["displayed-group"];
  expect(pending.tabs.map((tab) => (tab.kind === "attachment" ? tab.index : -1))).toEqual([
    1, 3, 4,
  ]);
  expect(pending.tabs.map((tab) => (tab.kind === "attachment" ? tab.media : ""))).toEqual([
    "image",
    "video",
    "markdown",
  ]);
  expect(pending.tabs[0]).toMatchObject({
    messageKey: "client:client-1",
    eventId: "",
    blobGroupId: "displayed-group",
    sender: "messageAuthorYou",
    sentAt: "2026-11-08T06:00:00Z",
    localPreviewUrl: "blob:image",
  });

  const reconciliation = reconcileCanonicalOutboxEvent(confirmed, "displayed-group");
  act(() => completeCanonicalOutboxReconciliation("displayed-group", reconciliation));
  await renderMessage(confirmed);
  click("[data-image-frame]"); // Reopening the now-confirmed attachment selects the same tab.
  const after = useResourcePanelStore.getState().groups["displayed-group"];
  expect(after.tabs).toHaveLength(3);
  expect(after.tabs.map((tab) => (tab.kind === "attachment" ? tab.blobPath : ""))).toEqual([
    "state/blobs/image",
    "state/blobs/video",
    "state/blobs/notes",
  ]);
  for (const tab of after.tabs) {
    expect(tab).toMatchObject({
      eventId: "event-1",
      blobGroupId: "blob-owner",
      sender: "messageAuthorYou",
      sentAt: "2026-11-08T06:00:01Z",
    });
    expect(tab.kind === "attachment" && tab.localPreviewUrl).toBeUndefined();
  }
  act(() => vi.advanceTimersByTime(60_000));
  expect(revoke).toHaveBeenCalledTimes(3);
});

it("keeps attachments from different authors with the same client id in separate tabs", async () => {
  const message = (id: string, by: string): LedgerEvent => ({
    id,
    by,
    kind: "chat.message",
    ts: "2026-11-08T06:00:00Z",
    data: {
      client_id: "shared-client-id",
      attachments: [{ path: `state/blobs/${id}`, title: "image.png", mime_type: "image/png" }],
    },
  });
  await renderMessage(message("event-a", "agent-a"));
  act(() => host.querySelector<HTMLButtonElement>("[data-image-frame]")!.click());
  await renderMessage(message("event-b", "agent-b"));
  act(() => host.querySelector<HTMLButtonElement>("[data-image-frame]")!.click());
  const tabs = useResourcePanelStore.getState().groups["displayed-group"].tabs;
  expect(tabs).toHaveLength(2);
  expect(tabs.map((tab) => (tab.kind === "attachment" ? tab.eventId : ""))).toEqual([
    "event-a",
    "event-b",
  ]);
});
