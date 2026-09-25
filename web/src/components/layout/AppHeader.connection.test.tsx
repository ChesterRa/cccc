// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { AppHeader, type AppHeaderProps } from "./AppHeader";
import { useUIStore } from "../../stores/useUIStore";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", changeLanguage: vi.fn() },
  }),
}));
const noop = () => {};
const props: AppHeaderProps = {
  theme: "dark",
  textScale: 100,
  onThemeChange: noop,
  onTextScaleChange: noop,
  selectedGroupId: "g",
  groupDoc: { group_id: "g", title: "基础手机调度平台", state: "idle" },
  selectedGroupRunning: true,
  selectedGroupRuntimeStatus: null,
  sseStatus: "connected",
  onOpenSidebar: noop,
  onOpenSearch: noop,
  onOpenContext: noop,
  onOpenSettings: noop,
  canAccessAccount: false,
  onOpenAccount: noop,
  onOpenMobileMenu: noop,
};
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  useUIStore.setState({ sseError: null });
});
it.each([false, true])(
  "uses the existing badge for connection changes (controls=%s)",
  async (controls) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    for (const status of ["disconnected", "connecting", "connected"] as const) {
      await act(async () =>
        root.render(
          <AppHeader
            {...props}
            sseStatus={status}
            onControlGroup={controls ? vi.fn() : undefined}
          />,
        ),
      );
      const badge = host.querySelector("[data-connection-state]")!;
      expect(badge).toBeTruthy();
      expect(host.querySelectorAll('[role="status"]')).toHaveLength(1);
      expect(host.querySelector("h1")?.parentElement?.querySelector("p")).toBeNull();
      expect(host.textContent).not.toContain("disconnected");
      expect(host.textContent).not.toContain("reconnecting");
      expect(badge.textContent).toBe("statusIdle");
      const dot = badge.firstElementChild!;
      if (status === "connected") {
        expect(dot.classList.contains("bg-rose-500")).toBe(false);
        expect(dot.classList.contains("animate-pulse")).toBe(false);
        expect(badge.getAttribute("title")).toBe("statusIdle");
      } else {
        expect(
          dot.classList.contains(status === "connecting" ? "animate-pulse" : "bg-rose-500"),
        ).toBe(true);
        expect(badge.getAttribute("aria-label")).toContain(
          status === "connecting" ? "reconnecting" : "disconnected",
        );
        if (controls)
          expect(
            host.querySelector("[data-group-run-controls]")?.getAttribute("aria-label"),
          ).toContain(status === "connecting" ? "reconnecting" : "disconnected");
      }
    }
  },
);
it("shows which call failed and the retry countdown when disconnected", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  useUIStore.setState({
    sseError: {
      endpoint: "ledger stream",
      kind: "http",
      status: 404,
      nextRetryAt: Date.now() + 7000,
    },
  });
  await act(async () => root.render(<AppHeader {...props} sseStatus="disconnected" />));
  const badge = host.querySelector("[data-connection-state]")!;
  const label = badge.getAttribute("aria-label") || "";
  expect(label).toContain("disconnected");
  expect(label).toContain("ledger stream HTTP 404");
  expect(label).toContain("retryingIn");
  // connected clears the detail text again
  useUIStore.setState({ sseError: null });
  await act(async () => root.render(<AppHeader {...props} sseStatus="connected" />));
  expect(badge.getAttribute("aria-label")).not.toContain("ledger stream");
});
