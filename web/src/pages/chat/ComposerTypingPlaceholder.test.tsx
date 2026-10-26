// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { ComposerTypingPlaceholder } from "./ComposerTypingPlaceholder";
import en from "../../i18n/locales/en/chat.json";
import zh from "../../i18n/locales/zh/chat.json";
import ja from "../../i18n/locales/ja/chat.json";

const keys = [
  "composerPromptTask",
  "composerPromptSkill",
  "composerPromptActor",
  "composerPromptGroup",
];
let host: HTMLDivElement;
let root: Root;
let media: EventTarget & { matches: boolean };
let hidden = false;
let language: ReturnType<typeof createInstance>;

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  media = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal("matchMedia", () => media);
  hidden = false;
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() =>
    hidden ? "hidden" : "visible",
  );
  language = createInstance();
  await language.init({
    lng: "test",
    fallbackLng: false,
    initAsync: false,
    resources: {
      test: { chat: Object.fromEntries(keys.map((key, i) => [key, ["ab", "cd", "ef", "gh"][i]])) },
      en: { chat: en },
      zh: { chat: zh },
      ja: { chat: ja },
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function render(focused = false) {
  await act(async () =>
    root.render(
      <I18nextProvider i18n={language}>
        <ComposerTypingPlaceholder focused={focused} fontSize={13} lineHeight={20} />
      </I18nextProvider>,
    ),
  );
}
async function tick(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
}
function text() {
  return host.querySelector("[data-composer-typing-text]")?.textContent;
}

describe("composer typing placeholder", () => {
  it("types four prompts, holds each complete sentence and loops", async () => {
    await render();
    expect(text()).toBe("");
    for (const word of ["ab", "cd", "ef", "gh", "ab"]) {
      await tick(60);
      expect(text()).toBe(word[0]);
      await tick(60);
      expect(text()).toBe(word);
      await tick(2999);
      expect(text()).toBe(word);
      await tick(1);
      expect(text()).toBe(word);
      await tick(30);
      expect(text()).toBe(word[0]);
      await tick(30);
      expect(text()).toBe("");
      await tick(30);
    }
    expect(host.firstElementChild?.getAttribute("aria-hidden")).toBe("true");
    expect(host.querySelector("[aria-live]")).toBeNull();
  });

  it("pauses on focus and resumes on blur", async () => {
    await render();
    await tick(60);
    await render(true);
    await tick(10000);
    expect(text()).toBe("a");
    await render(false);
    await tick(60);
    expect(text()).toBe("ab");
  });

  it("pauses in the background without catching up elapsed characters", async () => {
    await render();
    await tick(60);
    await act(async () => {
      hidden = true;
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await tick(10000);
    expect(text()).toBe("a");
    await act(async () => {
      hidden = false;
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await tick(60);
    expect(text()).toBe("ab");
  });

  it.each(["zh", "en", "ja"])(
    "shows a complete static first prompt for reduced motion in %s",
    async (locale) => {
      media.matches = true;
      await language.changeLanguage(locale);
      await render();
      expect(text()).toBe(language.t(keys[0], { ns: "chat" }));
      await tick(10000);
      expect(text()).toBe(language.t(keys[0], { ns: "chat" }));
      expect(vi.getTimerCount()).toBe(0);
    },
  );

  it("responds to reduced-motion changes and language changes without mixing text", async () => {
    await render();
    await tick(60);
    await act(async () => {
      media.matches = true;
      media.dispatchEvent(new Event("change"));
    });
    expect(text()).toBe("ab");
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => {
      await language.changeLanguage("zh");
    });
    expect(text()).toBe(zh.composerPromptTask);
    await act(async () => {
      media.matches = false;
      media.dispatchEvent(new Event("change"));
    });
    await tick(60);
    expect(text()).toBe("说");
  });

  it("cancels the pending frame when the hint unmounts", async () => {
    await render();
    expect(vi.getTimerCount()).toBe(1);
    await act(async () => root.render(null));
    expect(vi.getTimerCount()).toBe(0);
  });
});
