// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useModalA11y } from "../../hooks/useModalA11y";

// One t for every render, as i18next gives: effects that depend on it must not rerun each time.
const i18n = vi.hoisted(() => ({ t: (key: string) => key, i18n: { language: "en" } }));
vi.mock("react-i18next", () => ({ useTranslation: () => i18n }));
const data = vi.hoisted(() => ({
  domains: [],
  error: "",
  hasMore: false,
  items: [],
  matchedQuery: null,
  loading: false,
  loadState: async () => null,
  refresh: async () => {},
  selectedId: "",
  setError: () => {},
  setSelectedId: () => {},
  sources: {},
  state: null,
  summaryStats: null,
  totalCount: 0,
}));
vi.mock("./useCapabilityCenterData", () => ({ useCapabilityCenterData: () => data }));
vi.mock("../../services/api", () => ({
  fetchGroup: async () => ({ ok: true, result: { group: { title: "Fixture Group" } } }),
}));
import { CapabilityCenterWorkspace } from "./CapabilityCenterWorkspace";

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

// Stands in for the settings or context panel the center opens from.
function Panel({ onClose, children }: { onClose: () => void; children?: ReactNode }) {
  const { modalRef } = useModalA11y(true, onClose);
  return (
    <div ref={modalRef}>
      <button id="open-center">Open capability center</button>
      {children}
    </div>
  );
}

it("closes on Escape before the panel below it and gives focus back to the button that opened it", async () => {
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  const closePanel = vi.fn();
  const closeCenter = vi.fn();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const show = (centerOpen: boolean) =>
    act(async () =>
      root.render(
        <>
          <Panel onClose={closePanel} />
          {centerOpen ? (
            <CapabilityCenterWorkspace isOpen onClose={closeCenter} groupId="g1" />
          ) : null}
        </>,
      ),
    );
  await show(false);
  const opener = host.querySelector<HTMLButtonElement>("#open-center")!;
  opener.focus();
  await show(true);
  const center = document.querySelector<HTMLElement>("[data-capability-center]")!;
  expect(center.getAttribute("role")).toBe("dialog");
  expect(center.getAttribute("aria-modal")).toBe("true");
  expect(center.contains(document.activeElement)).toBe(true);

  await act(async () => {
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  });
  expect(closeCenter).toHaveBeenCalledTimes(1);
  expect(closePanel).not.toHaveBeenCalled();

  await show(false);
  expect(document.activeElement).toBe(opener);
  await act(async () => root.unmount());
});
