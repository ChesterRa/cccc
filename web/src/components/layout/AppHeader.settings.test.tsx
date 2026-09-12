// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { AppHeader, type AppHeaderProps } from "./AppHeader";
import { useModalA11y } from "../../hooks/useModalA11y";
import type { TextScale, Theme } from "../../types";

const { changeLanguage } = vi.hoisted(() => ({ changeLanguage: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en", changeLanguage },
  }),
}));

const noop = () => undefined;
const props: AppHeaderProps = {
  theme: "light",
  textScale: 100,
  onThemeChange: noop,
  onTextScaleChange: noop,
  selectedGroupId: "group-1",
  groupDoc: null,
  selectedGroupRunning: true,
  selectedGroupRuntimeStatus: null,
  actors: [],
  sseStatus: "connected",
  busy: "",
  onOpenSidebar: noop,
  onOpenGroupEdit: noop,
  onOpenSearch: noop,
  onOpenContext: noop,
  onStartGroup: noop,
  onStopGroup: noop,
  onSetGroupState: noop,
  onOpenSettings: noop,
  canAccessAccount: true,
  onOpenAccount: noop,
  onOpenMobileMenu: noop,
};
let root: Root;
let host: HTMLDivElement;

async function mount(overrides: Partial<AppHeaderProps> = {}) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<AppHeader {...props} {...overrides} />));
}
async function openMenu() {
  await act(async () =>
    host.querySelector<HTMLButtonElement>("[data-app-settings-trigger]")!.click(),
  );
  return document.querySelector<HTMLElement>("[data-app-settings-menu]")!;
}
function buttonByText(panel: Element, text: string): HTMLButtonElement {
  return Array.from(panel.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.trim() === text,
  )!;
}
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.clearAllMocks();
});

describe("header settings menu", () => {
  it("retains group shortcuts and routes account through the menu", async () => {
    const onOpenAccount = vi.fn();
    const onOpenContext = vi.fn();
    const onOpenGroupEdit = vi.fn();
    await mount({ onOpenAccount, onOpenContext, onOpenGroupEdit });
    await act(async () => {
      host.querySelector<HTMLButtonElement>('[aria-label="context"]')!.click();
      host.querySelector<HTMLButtonElement>('[aria-label="editGroup"]')!.click();
    });
    expect(onOpenContext).toHaveBeenCalledOnce();
    expect(onOpenGroupEdit).toHaveBeenCalledOnce();
    expect(host.querySelector('[aria-label="account"]')).toBeNull();
    const panel = await openMenu();
    await act(async () => buttonByText(panel, "account").click());
    expect(onOpenAccount).toHaveBeenCalledOnce();
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
  });

  it("keeps browser preferences usable without granting settings or account access", async () => {
    await mount({ canAccessAccount: false, selectedGroupId: "" });
    const panel = await openMenu();
    expect(
      panel.querySelectorAll('[data-appearance-preferences] button[aria-haspopup="menu"]'),
    ).toHaveLength(3);
    expect(buttonByText(panel, "account")).toBeUndefined();
    expect(buttonByText(panel, "settingsButton").disabled).toBe(true);
    await act(async () => root.render(<AppHeader {...props} canAccessAccount={false} />));
    const scoped = await openMenu();
    expect(buttonByText(scoped, "account")).toBeUndefined();
    expect(buttonByText(scoped, "settingsButton").disabled).toBe(false);
    await act(async () => root.render(<AppHeader {...props} webReadOnly />));
    expect(host.querySelector("[data-app-settings-trigger]")).toBeNull();
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
  });

  it("selects exact preference values without closing the panel", async () => {
    await mount();
    function Fixture() {
      const [theme, setTheme] = useState<Theme>("system");
      const [scale, setScale] = useState<TextScale>(100);
      return (
        <AppHeader
          {...props}
          theme={theme}
          textScale={scale}
          onThemeChange={setTheme}
          onTextScaleChange={setScale}
        />
      );
    }
    await act(async () => root.render(<Fixture />));
    const panel = await openMenu();
    for (const [label, text] of [
      ["themeLabel", "themeDark"],
      ["textSizeLabel", "125%"],
      ["common:language", "日本語"],
    ]) {
      const trigger = panel.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
      expect(document.querySelector("[data-appearance-menu]")).toBeNull();
      await act(async () => trigger.click());
      const menu = await vi.waitFor(() => {
        const element = document.getElementById(trigger.getAttribute("aria-controls")!);
        expect(element?.getAttribute("data-state")).toBe("open");
        return element!;
      });
      await act(async () => buttonByText(menu, text).click());
      await vi.waitFor(() => {
        expect(document.getElementById(menu.id)).toBeNull();
        expect(document.activeElement).toBe(trigger);
        expect(document.querySelector("[data-app-settings-menu]")).toBe(panel);
      });
    }
    expect(panel.querySelector('button[aria-label="themeLabel"]')!.textContent).toContain(
      "themeDark",
    );
    expect(panel.querySelector('button[aria-label="textSizeLabel"]')!.textContent).toContain(
      "125%",
    );
    expect(changeLanguage).toHaveBeenCalledWith("ja");
    expect(document.querySelector("[data-app-settings-menu]")).toBe(panel);
  });

  it("does not let a closing choice steal focus from a newly opened choice", async () => {
    await mount();
    const panel = await openMenu();
    vi.useFakeTimers();
    try {
      const theme = panel.querySelector<HTMLButtonElement>('[data-appearance-select="theme"]')!;
      await act(async () => theme.click());
      const themeMenu = document.getElementById(theme.getAttribute("aria-controls")!)!;
      await act(async () => buttonByText(themeMenu, "themeDark").click());
      const scale = panel.querySelector<HTMLButtonElement>('[data-appearance-select="textScale"]')!;
      await act(async () => scale.click());
      const scaleMenu = document.getElementById(scale.getAttribute("aria-controls")!)!;
      expect(scaleMenu).not.toBeNull();
      await act(async () => {
        vi.runOnlyPendingTimers();
      });
      expect(scale.getAttribute("aria-expanded")).toBe("true");
      expect(scaleMenu.contains(document.activeElement)).toBe(true);
      expect(document.querySelector("[data-app-settings-menu]")).toBe(panel);
    } finally {
      vi.useRealTimers();
    }
  });

  it("hands focus to the settings dialog and returns to the stable header trigger", async () => {
    await mount();
    function Fixture() {
      const [open, setOpen] = useState(false);
      const { modalRef } = useModalA11y(open, () => setOpen(false));
      return (
        <>
          <AppHeader {...props} onOpenSettings={() => setOpen(true)} />
          {open ? (
            <div role="dialog" ref={modalRef} data-test-settings>
              <button onClick={() => setOpen(false)}>Close settings</button>
            </div>
          ) : null}
        </>
      );
    }
    await act(async () => root.render(<Fixture />));
    const panel = await openMenu();
    await act(async () => buttonByText(panel, "settingsButton").click());
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    const dialog = host.querySelector("[data-test-settings]")!;
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
    await act(async () => dialog.querySelector<HTMLButtonElement>("button")!.click());
    expect(document.activeElement).toBe(host.querySelector("[data-app-settings-trigger]"));
  });
});
