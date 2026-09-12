// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { TextScale, Theme } from "../../types";
import { AppSettingsMenu } from "./AppSettingsMenu";
import { MobileMenuSheet } from "./MobileMenuSheet";

const { changeLanguage } = vi.hoisted(() => ({ changeLanguage: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en", changeLanguage },
  }),
}));
const pause = () => new Promise((resolve) => setTimeout(resolve, 30));
const noop = () => undefined;

describe("appearance choices inside their real hosts", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onTheme = vi.fn();
  const onScale = vi.fn();
  const onClose = vi.fn();
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });
  afterEach(async () => {
    await act(async () => {
      root.unmount();
      await pause();
    });
    container.remove();
  });
  async function renderHost(host: "desktop" | "sheet", initialTheme: Theme) {
    function Host() {
      const [theme, setTheme] = useState(initialTheme);
      const [textScale, setScale] = useState<TextScale>(100);
      const appearance = {
        theme,
        textScale,
        onThemeChange: (next: Theme) => {
          onTheme(next);
          setTheme(next);
        },
        onTextScaleChange: (next: TextScale) => {
          onScale(next);
          setScale(next);
        },
      };
      return host === "desktop" ? (
        <AppSettingsMenu
          {...appearance}
          canAccessAccount
          canOpenSettings
          onOpenAccount={noop}
          onOpenSettings={noop}
        />
      ) : (
        <MobileMenuSheet
          {...appearance}
          isOpen
          onClose={onClose}
          selectedGroupId=""
          groupDoc={null}
          selectedGroupRunning={false}
          actors={[]}
          busy=""
          onOpenSearch={noop}
          onOpenContext={noop}
          onOpenSettings={noop}
          canAccessAccount
          onOpenAccount={noop}
          onStartGroup={noop}
          onStopGroup={noop}
          onSetGroupState={noop}
        />
      );
    }
    await act(async () => {
      root.render(<Host />);
      await pause();
    });
    if (host === "desktop")
      await click(document.querySelector<HTMLButtonElement>("[data-app-settings-trigger]")!);
  }
  async function click(element: HTMLElement) {
    await act(async () => {
      element.click();
      await pause();
    });
  }
  function outerOpen(host: string) {
    return host === "desktop"
      ? !!document.querySelector("[data-app-settings-menu]")
      : !!document.querySelector(".mobile-menu-panel");
  }
  for (const host of ["desktop", "sheet"] as const) {
    for (const theme of ["light", "dark"] as const) {
      it.each([
        {
          label: "themeLabel",
          options: ["themeSystem", "themeLight", "themeDark"],
          pick: "themeSystem",
          value: "system",
        },
        {
          label: "textSizeLabel",
          options: ["70%", "90%", "100%", "125%"],
          pick: "125%",
          value: 125,
        },
        {
          label: "common:language",
          options: ["English", "中文", "日本語"],
          pick: "日本語",
          value: "ja",
        },
      ])(
        `${host}/${theme}: lists and selects $label without dismissing its host`,
        async ({ label, options, pick, value }) => {
          await renderHost(host, theme);
          const trigger = document.querySelector<HTMLButtonElement>(
            `[data-appearance-preferences] button[aria-label="${label}"]`,
          )!;
          await click(trigger);
          const items = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')];
          expect(items.map((item) => item.textContent!.replace("✓", "").trim())).toEqual(options);
          const checked = items.filter((item) => item.getAttribute("aria-checked") === "true");
          expect(checked).toHaveLength(1);
          expect(checked[0].textContent).toContain(
            label === "themeLabel"
              ? `theme${theme === "dark" ? "Dark" : "Light"}`
              : label === "textSizeLabel"
                ? "100%"
                : "English",
          );
          expect(document.activeElement).toBe(checked[0]);
          await click(items.find((item) => item.textContent!.includes(pick))!);
          expect(
            label === "themeLabel" ? onTheme : label === "textSizeLabel" ? onScale : changeLanguage,
          ).toHaveBeenCalledWith(value);
          expect(document.querySelector('[role="menuitemradio"]')).toBeNull();
          expect(outerOpen(host)).toBe(true);
          expect(onClose).not.toHaveBeenCalled();
          expect(document.activeElement).toBe(trigger);
        },
      );
    }
    it(`${host}: arrows move focus and Escape closes only the inner menu`, async () => {
      await renderHost(host, "light");
      const trigger = document.querySelector<HTMLButtonElement>(
        '[data-appearance-preferences] button[aria-label="themeLabel"]',
      )!;
      await click(trigger);
      await act(async () => {
        document.activeElement!.dispatchEvent(
          new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
        );
      });
      expect(document.activeElement?.textContent).toContain("themeDark");
      await act(async () => {
        document.activeElement!.dispatchEvent(
          new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
        );
        await pause();
      });
      expect(document.querySelector('[role="menuitemradio"]')).toBeNull();
      expect(outerOpen(host)).toBe(true);
      expect(onClose).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(trigger);
    });
  }
});
