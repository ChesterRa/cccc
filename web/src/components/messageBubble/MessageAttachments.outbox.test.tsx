// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useChatOutboxStore } from "../../stores/chatOutboxStore";
import type { LedgerEvent, MessageAttachment } from "../../types";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "../../utils/chatOutboxReconciliation";
import { MessageAttachments } from "./MessageAttachments";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  useChatOutboxStore.getState().clearAll();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it("plays and downloads a sent video from the stored blob after its local preview is revoked", () => {
  vi.useFakeTimers();
  const revoked: string[] = [];
  vi.spyOn(URL, "revokeObjectURL").mockImplementation((url) => {
    revoked.push(String(url));
  });
  // The production send path: optimistic copy, then the confirmed ledger event.
  useChatOutboxStore
    .getState()
    .enqueue("g1", "client-1", {
      id: "client-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        attachments: [
          {
            kind: "file",
            path: "",
            title: "clip.mp4",
            mime_type: "video/mp4",
            local_preview_url: "blob:video-preview",
          },
        ],
      },
    } as LedgerEvent);
  const reconciliation = reconcileCanonicalOutboxEvent(
    {
      id: "event-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        attachments: [
          {
            kind: "file",
            path: "state/blobs/video-hash",
            title: "clip.mp4",
            mime_type: "video/mp4",
          },
        ],
      },
    } as LedgerEvent,
    "g1",
  );
  completeCanonicalOutboxReconciliation("g1", reconciliation);
  vi.advanceTimersByTime(60_000);
  expect(revoked).toEqual(["blob:video-preview"]);

  const attachments = (reconciliation.event.data as { attachments: MessageAttachment[] })
    .attachments;
  act(() =>
    root.render(
      <MessageAttachments
        attachments={attachments}
        blobGroupId="g1"
        isUserMessage
        isDark={false}
        attachmentKeyPrefix="event-1"
        downloadTitle={(name) => `Download ${name}`}
      />,
    ),
  );
  act(() => {
    host.querySelector<HTMLButtonElement>('button[aria-label="playVideo"]')!.click();
  });

  const source = document.querySelector('[role="dialog"] video')!.getAttribute("src")!;
  expect(source).toContain("/api/v1/groups/g1/blobs/video-hash?filename=clip.mp4");
  const download = host.querySelector<HTMLAnchorElement>('a[download="clip.mp4"]')!;
  expect(download.getAttribute("href")).toContain("/api/v1/groups/g1/blobs/video-hash?");
  expect(document.body.innerHTML).not.toContain("blob:video-preview");
});
