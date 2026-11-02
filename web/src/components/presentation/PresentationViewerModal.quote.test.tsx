// @vitest-environment happy-dom
import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { PresentationViewerModal } from "./PresentationViewerModal";
import type { GroupPresentation } from "../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

// A live browser hands the viewer a picture of the page, which Quote uploads as a snapshot.
vi.mock("./PresentationWebPreviewPanel", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./PresentationWebPreviewPanel")>()),
  PresentationWebPreviewPanel: ({
    onInteractiveFrameUpdate,
  }: {
    onInteractiveFrameUpdate?: (frame: unknown) => void;
  }) => {
    useEffect(() => {
      onInteractiveFrameUpdate?.({
        dataUrl: "data:image/png;base64,AAAA",
        url: "https://example.com/checkout",
        capturedAt: "2026-10-31T08:00:00Z",
        width: 2,
        height: 2,
      });
    }, [onInteractiveFrameUpdate]);
    return null;
  },
}));

let finishUpload: (value: unknown) => void = () => {};
vi.mock("../../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/api")>()),
  fetchPresentationBrowserSurfaceSession: vi.fn(async () => ({ ok: false })),
  uploadPresentationReferenceSnapshot: vi.fn(
    () =>
      new Promise((resolve) => {
        finishUpload = resolve;
      }),
  ),
}));

let host: HTMLDivElement, root: ReturnType<typeof createRoot>;

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(new Blob(["png"], { type: "image/png" }))),
  );
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("keeps the phone Quote key's width while a snapshot uploads, with a spinner and the pending name", async () => {
  const presentation: GroupPresentation = {
    v: 1,
    slots: [
      {
        slot_id: "slot-1",
        index: 1,
        card: {
          slot_id: "slot-1",
          title: "Checkout page",
          card_type: "web_preview",
          published_at: "one",
          published_by: "worker",
          content: { mode: "reference", url: "https://example.com/checkout" },
        },
      },
    ],
  };
  const onQuoteInChat = vi.fn();
  await act(async () =>
    root.render(
      <PresentationViewerModal
        isOpen
        isDark={false}
        groupId="g"
        slotId="slot-1"
        presentation={presentation}
        onClose={vi.fn()}
        onQuoteInChat={onQuoteInChat}
      />,
    ),
  );
  const rowKey = () =>
    host.ownerDocument.querySelector<HTMLButtonElement>(
      "[data-presentation-details] button[data-presentation-action]",
    )!;
  expect(rowKey().getAttribute("aria-label")).toBe("presentationQuoteInChatAction");

  vi.useFakeTimers();
  await act(async () => rowKey().click());
  const pending = rowKey();
  expect(pending.disabled).toBe(true);
  expect(pending.getAttribute("aria-label")).toBe("presentationQuotePendingAction");
  await act(async () => vi.advanceTimersByTime(250));
  expect(pending.querySelector(".lcd-spinner[data-shown]")).not.toBeNull();
  // The visible label stays, so the row does not rewrap under the finger.
  expect(pending.textContent).toBe("presentationQuoteAction");

  await act(async () =>
    finishUpload({ ok: true, result: { snapshot: { path: "state/blobs/s.png" } } }),
  );
  expect(rowKey().disabled).toBe(false);
  expect(rowKey().querySelector(".lcd-spinner")).toBeNull();
  expect(onQuoteInChat).toHaveBeenCalledWith(expect.objectContaining({ slotId: "slot-1" }));
});
