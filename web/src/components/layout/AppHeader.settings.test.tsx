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
      panel.querySelectorAll('[data-appearance-preferences] [role="radiogroup"]'),
    ).toHaveLength(3);
    expect(buttonByText(panel, "account")).toBeUndefined();
    expect(buttonByText(panel, "allSettings").disabled).toBe(true);
    await act(async () => root.render(<AppHeader {...props} canAccessAccount={false} />));
    const scoped = await openMenu();
    expect(buttonByText(scoped, "account")).toBeUndefined();
    expect(buttonByText(scoped, "allSettings").disabled).toBe(false);
    await act(async () => root.render(<AppHeader {...props} webReadOnly />));
    expect(host.querySelector("[data-app-settings-trigger]")).toBeNull();
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
  });

  it("applies each preference in one tap and keeps the panel open", async () => {
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
    const choice = (name: string) =>
      panel.querySelector<HTMLElement>(`[data-appearance-choice="${name}"]`)!;
    const checked = (name: string) =>
      choice(name).querySelector('[role="radio"][aria-checked="true"]')?.textContent;
    expect(checked("theme")).toBe("themeSystem");
    expect(checked("textScale")).toBe("100%");
    await act(async () => buttonByText(choice("theme"), "themeDark").click());
    await act(async () => buttonByText(choice("textScale"), "125%").click());
    await act(async () => buttonByText(choice("language"), "日本語").click());
    expect(checked("theme")).toBe("themeDark");
    expect(checked("textScale")).toBe("125%");
    expect(changeLanguage).toHaveBeenCalledWith("ja");
    expect(document.querySelector("[data-app-settings-menu]")).toBe(panel);
  });

  it("moves a choice's selection with the arrow keys", async () => {
    await mount();
    function Fixture() {
      const [theme, setTheme] = useState<Theme>("light");
      return <AppHeader {...props} theme={theme} onThemeChange={setTheme} />;
    }
    await act(async () => root.render(<Fixture />));
    const panel = await openMenu();
    const theme = panel.querySelector<HTMLElement>('[data-appearance-choice="theme"]')!;
    const radios = [...theme.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    expect(radios.map((radio) => radio.tabIndex)).toEqual([0, -1, -1]);
    await act(async () => {
      radios[0].focus();
      radios[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    });
    expect(radios[1].getAttribute("aria-checked")).toBe("true");
    expect(document.activeElement).toBe(radios[1]);
    await act(async () => {
      radios[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    });
    expect(radios[2].getAttribute("aria-checked")).toBe("true");
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
    await act(async () => buttonByText(panel, "allSettings").click());
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    const dialog = host.querySelector("[data-test-settings]")!;
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(document.querySelector("[data-app-settings-menu]")).toBeNull();
    await act(async () => dialog.querySelector<HTMLButtonElement>("button")!.click());
    expect(document.activeElement).toBe(host.querySelector("[data-app-settings-trigger]"));
  });

  it("opens the menu on a mouse click instead of jumping into settings", async () => {
    const onOpenSettings = vi.fn();
    await mount({ onOpenSettings });
    const trigger = host.querySelector<HTMLButtonElement>("[data-app-settings-trigger]")!;
    await mouseClick(trigger);
    expect(onOpenSettings).not.toHaveBeenCalled();
    const panel = document.querySelector<HTMLElement>("[data-app-settings-menu]")!;
    expect(panel).not.toBeNull();
    await act(async () => buttonByText(panel, "allSettings").click());
    expect(onOpenSettings).toHaveBeenCalledOnce();
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

it("does not open the menu on hover, leaving focus where it was", async () => {
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
      await vi.advanceTimersByTimeAsync(600);
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

it.each([
  ["active", "statusOffline", ["launchAllAgents", "stopAllAgents"]],
  ["stopped", "statusStopped", ["launchAllAgents"]],
] as const)(
  "names a Group with no running Actor and state %s as %s",
  async (state, word, labels) => {
    await mount({
      groupDoc: { group_id: "group-1", state },
      selectedGroupRunning: false,
      onControlGroup: vi.fn(),
    });
    const trigger = host.querySelector<HTMLButtonElement>("[data-group-run-controls]")!;
    expect(trigger.textContent).toContain(word);
    await act(async () => trigger.click());
    const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
    expect(
      Array.from(menu.querySelectorAll('[role="menuitem"]')).map((item) =>
        item.textContent?.trim(),
      ),
    ).toEqual(labels);
  },
);

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
