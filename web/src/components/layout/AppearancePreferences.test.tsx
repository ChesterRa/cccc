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
          onOpenSearch={noop}
          onOpenContext={noop}
          onOpenSettings={noop}
          canAccessAccount
          onOpenAccount={noop}
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
          name: "theme",
          options: ["themeLight", "themeDark", "themeSystem"],
          pick: "themeSystem",
          value: "system",
        },
        { name: "textScale", options: ["70%", "90%", "100%", "125%"], pick: "125%", value: 125 },
        { name: "language", options: ["中文", "English", "日本語"], pick: "日本語", value: "ja" },
      ])(
        `${host}/${theme}: shows every $name option and applies one in a tap without dismissing its host`,
        async ({ name, options, pick, value }) => {
          await renderHost(host, theme);
          const group = document.querySelector<HTMLElement>(
            `[data-appearance-preferences] [data-appearance-choice="${name}"]`,
          )!;
          expect(group.getAttribute("role")).toBe("radiogroup");
          const radios = [...group.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
          expect(radios.map((radio) => radio.textContent)).toEqual(options);
          const checked = radios.filter((radio) => radio.getAttribute("aria-checked") === "true");
          expect(checked.map((radio) => radio.textContent)).toEqual([
            name === "theme"
              ? `theme${theme === "dark" ? "Dark" : "Light"}`
              : name === "textScale"
                ? "100%"
                : "English",
          ]);
          await click(radios.find((radio) => radio.textContent === pick)!);
          expect(
            name === "theme" ? onTheme : name === "textScale" ? onScale : changeLanguage,
          ).toHaveBeenCalledWith(value);
          expect(outerOpen(host)).toBe(true);
          expect(onClose).not.toHaveBeenCalled();
        },
      );
    }
  }
});
