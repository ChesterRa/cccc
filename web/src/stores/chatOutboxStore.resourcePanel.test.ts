import { afterEach, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../types";
import { useChatOutboxStore } from "./chatOutboxStore";
import {
  resourceKey,
  useResourcePanelStore,
  type AttachmentResource,
} from "./useResourcePanelStore";

afterEach(() => {
  useChatOutboxStore.getState().clearAll();
  useResourcePanelStore.setState({ groups: {} });
  vi.restoreAllMocks();
});

it("keeps failed-send previews available until open and recently closed tabs release them", () => {
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  const attachment = (index: number): AttachmentResource => ({
    kind: "attachment",
    messageKey: "client:send-1",
    eventId: "",
    index,
    blobGroupId: "g1",
    blobPath: "",
    localPreviewUrl: `blob:preview-${index}`,
    media: "image",
    isSvg: false,
  });
  const one = attachment(0);
  const two = attachment(1);
  const panel = useResourcePanelStore.getState();
  panel.open("g1", one, "one.png");
  panel.open("g1", two, "two.png");
  panel.closeTabs("g1", [resourceKey(two)]);
  useChatOutboxStore
    .getState()
    .enqueue("g1", "send-1", {
      id: "send-1",
      kind: "chat.message",
      by: "user",
      data: {
        attachments: [
          { local_preview_url: "blob:preview-0" },
          { local_preview_url: "blob:preview-1" },
        ],
      },
    } as LedgerEvent);

  // This is the failed-send path: the composer gets its original files back and the outbox
  // releases its ownership, but both a visible tab and a recently closed tab still use the URLs.
  useChatOutboxStore.getState().remove("g1", "send-1");
  expect(revoke).not.toHaveBeenCalled();
  const entry = useResourcePanelStore.getState().groups.g1.closed[0];
  panel.restore("g1", entry.id);
  expect(revoke).not.toHaveBeenCalled();
  panel.closeTabs("g1", [resourceKey(two)]);
  expect(revoke).not.toHaveBeenCalled();

  // A late confirmation replaces one source, releasing only the URL no longer referenced.
  panel.updateAttachment("g1", "client:send-1", 0, {
    eventId: "event-1",
    blobPath: "state/blobs/one",
    localPreviewUrl: undefined,
  });
  expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:preview-0");

  // Closed entries have a bounded history. Once this one falls out, the last owner releases it.
  for (let i = 0; i < 20; i += 1) {
    const next = { ...attachment(i + 2), messageKey: `event:${i}`, localPreviewUrl: undefined };
    panel.open("g1", next, `other-${i}.png`);
    panel.closeTabs("g1", [resourceKey(next)]);
  }
  expect(revoke).toHaveBeenCalledTimes(2);
  expect(revoke).toHaveBeenLastCalledWith("blob:preview-1");
});

it("releases unreferenced outbox previews on page cleanup but preserves a restorable tab", () => {
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  useResourcePanelStore
    .getState()
    .open(
      "g1",
      {
        kind: "attachment",
        messageKey: "client:send-2",
        eventId: "",
        index: 0,
        blobGroupId: "g1",
        blobPath: "",
        localPreviewUrl: "blob:kept-by-tab",
        media: "image",
        isSvg: false,
      },
      "kept.png",
    );
  useChatOutboxStore
    .getState()
    .enqueue("g1", "send-2", {
      id: "send-2",
      kind: "chat.message",
      by: "user",
      data: {
        attachments: [
          { local_preview_url: "blob:kept-by-tab" },
          { local_preview_url: "blob:not-opened" },
        ],
      },
    } as LedgerEvent);

  // App's pagehide handler clears the outbox; a bfcache-restored tab remains an owner.
  useChatOutboxStore.getState().clearAll();
  expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:not-opened");
  expect(useResourcePanelStore.getState().groups.g1.tabs).toHaveLength(1);
  useResourcePanelStore.setState({ groups: {} });
  expect(revoke).toHaveBeenCalledTimes(2);
  expect(revoke).toHaveBeenLastCalledWith("blob:kept-by-tab");
});
