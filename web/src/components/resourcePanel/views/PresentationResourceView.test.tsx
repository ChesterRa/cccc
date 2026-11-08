// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { GroupPresentation, PresentationMessageRef } from "../../../types";
import { PresentationResourceView } from "./PresentationResourceView";
import { useModalStore } from "../../../stores/useModalStore";

const preview = vi.hoisted(() => ({ mount: vi.fn(), unmount: vi.fn(), props: vi.fn() }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("../../presentation/PresentationWebPreviewPanel", async () => {
  const { useEffect } = await import("react");
  return {
    PresentationWebPreviewPanel: (props: { visible?: boolean; reuseExistingSession?: boolean }) => {
      preview.props(props);
      useEffect(() => {
        preview.mount();
        return () => preview.unmount();
      }, []);
      return <div data-test-shared-viewer />;
    },
  };
});
vi.mock("../../../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../services/api")>()),
  fetchPresentationBrowserSurfaceSession: vi.fn(async () => ({ ok: false })),
}));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  preview.mount.mockClear();
  preview.unmount.mockClear();
  preview.props.mockClear();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const presentation: GroupPresentation = {
  v: 1,
  slots: [
    {
      slot_id: "slot-1",
      index: 1,
      card: {
        slot_id: "slot-1",
        title: "Shared web page",
        card_type: "web_preview",
        published_at: "2026-11-08T00:00:00Z",
        published_by: "fixture",
        content: { mode: "inline", url: "https://example.test/" },
      },
    },
  ],
};

const ref = (path: string): PresentationMessageRef => ({
  kind: "presentation_ref",
  slot_id: "slot-1",
  card_type: "web_preview",
  href: "https://example.test/",
  snapshot: { path },
});

it("keeps the shared viewer mounted as the selected reference and visibility change", async () => {
  const jump = vi.fn();
  const openWindow = vi.fn();
  const props = {
    groupId: "g",
    slotId: "slot-1",
    presentation,
    isDark: false,
    handlers: { onOpenMessageContext: jump, onOpenWindow: openWindow },
  };
  await act(async () =>
    root.render(
      <PresentationResourceView
        {...props}
        visible
        focusEventId="event-a"
        focusRef={ref("state/blobs/a.png")}
      />,
    ),
  );
  const viewer = host.querySelector("[data-test-shared-viewer]");
  expect(viewer).not.toBeNull();
  expect(preview.mount).toHaveBeenCalledTimes(1);
  expect(preview.props).toHaveBeenLastCalledWith(
    expect.objectContaining({ visible: true, reuseExistingSession: true }),
  );
  expect(host.querySelector("[data-side-panel-header]")).toBeNull();
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="presentationJumpToChatAction"]')!.click(),
  );
  expect(jump).toHaveBeenCalledWith("event-a");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="presentationOpenWindowAction"]')!.click(),
  );
  expect(openWindow).toHaveBeenCalledOnce();

  await act(async () =>
    root.render(
      <PresentationResourceView
        {...props}
        visible={false}
        focusEventId="event-a"
        focusRef={ref("state/blobs/a.png")}
      />,
    ),
  );
  expect(preview.props).toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }));
  await act(async () =>
    root.render(
      <PresentationResourceView
        {...props}
        visible
        focusEventId="event-b"
        focusRef={ref("state/blobs/b.png")}
      />,
    ),
  );
  expect(host.querySelector("[data-test-shared-viewer]")).toBe(viewer);
  expect(preview.mount).toHaveBeenCalledTimes(1);
  expect(preview.unmount).not.toHaveBeenCalled();
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="presentationJumpToChatAction"]')!.click(),
  );
  expect(jump).toHaveBeenLastCalledWith("event-b");
});

it.each([false, true])(
  "keeps an old snapshot accessible when the shared slot is empty (short=%s)",
  async (short) => {
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query) =>
        ({
          matches: short && query === "(max-height: 480px)",
          media: query,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
        }) as unknown as MediaQueryList,
    );
    await act(async () =>
      root.render(
        <PresentationResourceView
          groupId="g"
          slotId="slot-1"
          presentation={{ v: 1, slots: [] }}
          focusRef={ref("state/blobs/old.png")}
          focusEventId="old-message"
          visible
          readOnly
          isDark={false}
          handlers={{}}
        />,
      ),
    );
    expect(host.textContent).toContain("presentationMissingCard");
    const snapshot = host.querySelector<HTMLButtonElement>(
      '[aria-label="presentationOpenQuotedSnapshotAction"]',
    );
    expect(snapshot).not.toBeNull();
    await act(async () => snapshot!.click());
    expect(host.querySelector("[data-presentation-scrim]")).not.toBeNull();
  },
);

it("clears attention only while this slot's resource view is visible", async () => {
  useModalStore.setState({ presentationAttention: {}, presentationViewer: null });
  const mark = () => useModalStore.getState().markPresentationSlotAttention("g", "slot-1");
  const hasAttention = () => !!useModalStore.getState().presentationAttention.g?.["slot-1"];
  const view = (visible: boolean) => (
    <PresentationResourceView
      groupId="g"
      slotId="slot-1"
      presentation={presentation}
      isDark={false}
      visible={visible}
      handlers={{}}
    />
  );
  act(mark);
  expect(hasAttention()).toBe(true);
  await act(async () => root.render(view(true)));
  expect(hasAttention()).toBe(false);
  act(mark);
  expect(hasAttention()).toBe(false);

  await act(async () => root.render(view(false)));
  act(mark);
  expect(hasAttention()).toBe(true);
  await act(async () => root.render(view(true)));
  expect(hasAttention()).toBe(false);
  await act(async () => root.render(null));
  act(mark);
  expect(hasAttention()).toBe(true);
});
