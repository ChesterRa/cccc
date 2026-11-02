// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { TextScale, Theme } from "../../types";
import { AppSettingsMenu } from "./AppSettingsMenu";
import { MobileMenuSheet } from "./MobileMenuSheet";
import { applyUiStylePrefs, getUiStylePrefs, setUiStylePrefs } from "../../utils/uiStyle";
import { useUIStore } from "../../stores/useUIStore";

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
    useUIStore.setState({ showPendingPrompts: true });
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
    useUIStore.setState({ showPendingPrompts: true });
    localStorage.removeItem("cccc-pending-prompts");
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
  async function openMore() {
    const more = document.querySelector<HTMLButtonElement>("[data-appearance-more]")!;
    if (more.getAttribute("aria-expanded") !== "true") await click(more);
  }
  for (const host of ["desktop", "sheet"] as const) {
    it(`${host}: keeps theme and language in view and folds the rest behind one key`, async () => {
      await renderHost(host, "light");
      const more = document.querySelector<HTMLButtonElement>("[data-appearance-more]")!;
      const folded = document.getElementById(more.getAttribute("aria-controls")!)!;
      const names = (root: ParentNode) =>
        [...root.querySelectorAll<HTMLElement>("[data-appearance-choice]")].map(
          (choice) => choice.dataset.appearanceChoice,
        );
      expect(more.getAttribute("aria-expanded")).toBe("false");
      expect(folded.hidden).toBe(true);
      expect(names(folded)).toEqual([
        "lcdBody",
        "lcdWidth",
        "lcdMotion",
        "pendingPrompts",
        "textScale",
      ]);
      const preferences = document.querySelector("[data-appearance-preferences]")!;
      expect(names(preferences).filter((name) => !names(folded).includes(name))).toEqual([
        "theme",
        "language",
      ]);
      // The key follows what stays in view, so opening it moves nothing above.
      const language = preferences.querySelector('[data-appearance-choice="language"]')!;
      expect(
        language.compareDocumentPosition(more) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      await click(more);
      expect(more.getAttribute("aria-expanded")).toBe("true");
      expect(folded.hidden).toBe(false);
      expect(outerOpen(host)).toBe(true);
    });

    it(`${host}: changes Waiting for you indicators without dismissing the menu`, async () => {
      await renderHost(host, "light");
      await openMore();
      const choice = document.querySelector<HTMLElement>(
        '[data-appearance-choice="pendingPrompts"]',
      )!;
      const radios = [...choice.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
      expect(radios.map((radio) => radio.textContent)).toEqual([
        "pendingPromptsShow",
        "pendingPromptsOff",
      ]);
      expect(radios[0].getAttribute("aria-checked")).toBe("true");
      await click(radios[1]);
      expect(useUIStore.getState().showPendingPrompts).toBe(false);
      expect(localStorage.getItem("cccc-pending-prompts")).toBe("false");
      expect(outerOpen(host)).toBe(true);
      await click(radios[0]);
      expect(useUIStore.getState().showPendingPrompts).toBe(true);
    });

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
          if (name === "textScale") await openMore();
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

  for (const host of ["desktop", "sheet"] as const) {
    it(`${host}: offers the body-font, width and motion choices, with no style switch`, async () => {
      await renderHost(host, "light");
      const choice = (name: string) =>
        document.querySelector<HTMLElement>(
          `[data-appearance-preferences] [data-appearance-choice="${name}"]`,
        );
      const radios = (name: string) => [
        ...(choice(name)?.querySelectorAll<HTMLButtonElement>('[role="radio"]') ?? []),
      ];
      expect(choice("uiStyle")).toBeNull();
      try {
        expect(radios("lcdBody").map((radio) => radio.textContent)).toEqual([
          "lcdBodyMono",
          "lcdBodyPixel",
        ]);
        await click(radios("lcdBody")[1]!);
        expect(document.documentElement.dataset.lcdBody).toBe("pixel");
        expect(radios("lcdWidth").map((radio) => radio.textContent)).toEqual([
          "lcdWidthColumn",
          "lcdWidthWide",
          "lcdWidthFull",
        ]);
        // Hidden where the CSS cap does not apply (below 768px).
        expect(choice("lcdWidth")!.parentElement!.className).toContain("max-md:hidden");
        await click(radios("lcdWidth")[1]!);
        expect(document.documentElement.dataset.lcdWidth).toBe("wide");
        await click(radios("lcdWidth")[2]!);
        expect(document.documentElement.dataset.lcdWidth).toBe("full");
        expect(radios("lcdMotion").map((radio) => radio.textContent)).toEqual([
          "lcdMotionOn",
          "lcdMotionOff",
        ]);
        await click(radios("lcdMotion")[1]!);
        expect(document.documentElement.dataset.lcdMotion).toBe("off");
        expect(document.querySelector("[data-lcd-motion-system]")).toBeNull();
        expect(outerOpen(host)).toBe(true);
      } finally {
        await act(async () =>
          setUiStylePrefs({ lcdBody: "mono", lcdWidth: "column", lcdMotion: "on" }),
        );
      }
    });
  }

  it("says under Motion when the system's reduced-motion setting keeps the LCD still", async () => {
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query: string) =>
        ({
          media: query,
          matches: query.includes("prefers-reduced-motion"),
          addEventListener: noop,
          removeEventListener: noop,
        }) as unknown as MediaQueryList,
    );
    try {
      await act(async () => applyUiStylePrefs(getUiStylePrefs()));
      await renderHost("sheet", "light");
      const hint = document.querySelector("[data-lcd-motion-system]");
      expect(hint?.textContent).toBe("lcdMotionSystemStill");
      // The stored choice stays the user's; only what shows is still.
      expect(document.documentElement.dataset.lcdMotion).toBe("off");
    } finally {
      vi.restoreAllMocks();
      await act(async () => applyUiStylePrefs(getUiStylePrefs()));
    }
  });
});
