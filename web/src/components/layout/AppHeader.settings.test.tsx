// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { AppHeader, type AppHeaderProps } from "./AppHeader";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useUIStore } from "../../stores/useUIStore";
import { useModalStore } from "../../stores/useModalStore";
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
  sseStatus: "connected",
  onOpenSidebar: noop,
  onOpenGroupEdit: noop,
  onOpenSearch: noop,
  onOpenContext: noop,
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
/** A real mouse click: a pointerdown that names the pointer, then a click with a detail count. */
async function mouseClick(element: HTMLElement) {
  await act(async () => {
    element.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "mouse" }));
    element.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
  });
}
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.clearAllMocks();
  useUIStore.setState({ chatSessions: {} });
  useModalStore.setState({ presentationAttention: {} });
});

describe("header settings menu", () => {
  it("retains group shortcuts and routes account through the menu", async () => {
    const onOpenAccount = vi.fn();
    const onOpenContext = vi.fn();
    const onOpenGroupEdit = vi.fn();
    await mount({
      onOpenAccount,
      onOpenContext,
      onOpenGroupEdit,
      groupDoc: { group_id: "g1", title: "Robots" },
    });
    expect(host.querySelector("h1")?.textContent).toBe("Robots");
    await act(async () => {
      host.querySelector<HTMLButtonElement>('[aria-label="context"]')!.click();
      host.querySelector<HTMLButtonElement>('[aria-label="editGroup"]')!.click();
    });
    expect(onOpenContext).toHaveBeenCalledOnce();
    expect(onOpenGroupEdit).toHaveBeenCalledOnce();
    expect(document.querySelector('[role="menu"][aria-label="groupActions"]')).toBeNull();
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

  it("opens settings on a mouse click and keeps the menu for every other input", async () => {
    const onOpenSettings = vi.fn();
    await mount({ onOpenSettings });
    const trigger = host.querySelector<HTMLButtonElement>("[data-app-settings-trigger]")!;
    await mouseClick(trigger);
    expect(onOpenSettings).toHaveBeenCalledOnce();
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();

    // A tap has no hover to fall back on, so it must still reach the menu.
    await act(async () => {
      trigger.dispatchEvent(
        new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }),
      );
      trigger.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    });
    expect(onOpenSettings).toHaveBeenCalledOnce();
    const panel = document.querySelector<HTMLElement>("[data-app-settings-menu]")!;
    expect(panel).not.toBeNull();
    // Settings stays in the menu because that is the only route a tap or keyboard has.
    await act(async () => buttonByText(panel, "settingsButton").click());
    expect(onOpenSettings).toHaveBeenCalledTimes(2);
  });

  it("keeps Group surfaces and their attention out of the global settings menu", async () => {
    useModalStore.setState({ presentationAttention: { "group-1": { "slot-1": true } } });
    await mount();
    expect(host.querySelector("[data-app-settings-attention]")).toBeNull();
    const panel = await openMenu();
    expect(panel.querySelector("[data-app-settings-presentation]")).toBeNull();
    expect(panel.textContent).not.toContain("groupConnections.title");
  });
});

it("preserves text-input focus when a hover-only menu closes", async () => {
  vi.useFakeTimers();
  const input = document.createElement("input");
  document.body.append(input);
  try {
    await mount();
    input.focus();
    const trigger = host.querySelector<HTMLButtonElement>("[data-app-settings-trigger]")!;
    await act(async () => {
      trigger.dispatchEvent(
        new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }),
      );
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });
    expect(document.querySelector("[data-app-settings-menu]")).not.toBeNull();
    expect(document.activeElement).toBe(input);
    await act(async () => {
      trigger.dispatchEvent(
        new PointerEvent("pointerout", {
          bubbles: true,
          pointerType: "mouse",
          relatedTarget: input,
        }),
      );
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
    expect(document.activeElement).toBe(input);
  } finally {
    input.remove();
    vi.useRealTimers();
  }
});

it("keeps the title readable without exposing editing when unavailable", async () => {
  await mount();
  expect(host.querySelector('[aria-label="editGroup"]')).not.toBeNull();
  for (const unavailable of [
    { webReadOnly: true },
    { selectedGroupId: "" },
    { onOpenGroupEdit: undefined },
  ]) {
    await act(async () => root.render(<AppHeader {...props} {...unavailable} />));
    expect(host.querySelector('[aria-label="editGroup"]')).toBeNull();
    expect(host.querySelector("h1")?.textContent).toBeTruthy();
    expect(host.querySelector("h1 button")).toBeNull();
  }
});

it("turns the status badge into the Group run menu", async () => {
  const onControlGroup = vi.fn();
  await mount({
    groupDoc: { group_id: "group-1", state: "active" },
    selectedGroupRunning: true,
    onControlGroup,
  });
  const trigger = host.querySelector<HTMLButtonElement>("[data-group-run-controls]")!;
  expect(trigger.textContent).toContain("statusRunning");
  await act(async () => trigger.click());
  const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
  const labels = Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) =>
    item.textContent?.trim(),
  );
  expect(labels).toEqual(["pauseDelivery", "stopAllAgents"]);
  await act(async () => menu.querySelector<HTMLButtonElement>('[role="menuitem"]')!.click());
  expect(onControlGroup).toHaveBeenCalledWith("group-1", "pause");
});

it("shows a plain status badge when the viewer cannot control the Group", async () => {
  await mount({
    groupDoc: { group_id: "group-1", state: "active" },
    selectedGroupRunning: true,
    webReadOnly: true,
    onControlGroup: vi.fn(),
  });
  expect(host.querySelector("[data-group-run-controls]")).toBeNull();
  expect(host.textContent).toContain("statusRunning");
});
