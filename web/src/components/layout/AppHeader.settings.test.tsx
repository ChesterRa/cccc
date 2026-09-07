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
    await act(async () => panel.querySelector<HTMLButtonElement>("button")!.click());
    expect(onOpenAccount).toHaveBeenCalledOnce();
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
  });

  it("keeps browser preferences usable without granting settings or account access", async () => {
    await mount({ canAccessAccount: false, selectedGroupId: "" });
    const panel = await openMenu();
    expect(panel.querySelectorAll("select")).toHaveLength(3);
    expect(panel.querySelectorAll("button")).toHaveLength(1);
    expect(panel.querySelector<HTMLButtonElement>("button")!.disabled).toBe(true);
    await act(async () => root.render(<AppHeader {...props} canAccessAccount={false} />));
    const scoped = await openMenu();
    expect(scoped.querySelectorAll("button")).toHaveLength(1);
    expect(scoped.querySelector<HTMLButtonElement>("button")!.disabled).toBe(false);
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
    const [theme, scale, language] = panel.querySelectorAll("select");
    for (const [select, value] of [
      [theme, "dark"],
      [scale, "125"],
      [language, "ja"],
    ] as const) {
      await act(async () => {
        select.value = value;
        select.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }
    expect(theme.value).toBe("dark");
    expect(scale.value).toBe("125");
    expect(changeLanguage).toHaveBeenCalledWith("ja");
    expect(document.querySelector("[data-app-settings-menu]")).toBe(panel);
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
    await act(async () => panel.querySelector<HTMLButtonElement>("button:last-child")!.click());
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
