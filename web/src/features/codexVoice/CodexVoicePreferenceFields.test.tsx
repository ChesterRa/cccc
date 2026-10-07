// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore } from "../../stores/useGroupStore";
import { CodexVoicePreferenceFields } from "./CodexVoicePreferenceFields";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number }) =>
      options?.count === undefined ? key : `${key}:${options.count}`,
  }),
}));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  useGroupStore.setState({
    groups: [
      { group_id: "g_a", title: "Alpha" },
      { group_id: "g_b", title: "Beta" },
    ] as never,
  });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render(section: "audio" | "notifications", groups: Record<string, string> = {}) {
  const change = vi.fn();
  const preferences = {
    value: { verbosity: "standard", style: "natural", suppress_viewed: true, groups },
    saving: false,
    saved: false,
    error: "",
    change,
  };
  act(() =>
    root.render(
      <CodexVoicePreferenceFields preferences={preferences as never} section={section} />,
    ),
  );
  return change;
}
const group = (name: string) =>
  host.querySelector<HTMLElement>(`[role="radiogroup"][aria-label="${name}"]`)!;
const option = (scope: HTMLElement, index: number) =>
  scope.querySelectorAll<HTMLButtonElement>('[role="radio"]')[index];

it("saves the chosen notification scope for exactly that Group", () => {
  const change = render("notifications", { g_b: "all_chat" });
  // Current scopes are shown, defaulting to off.
  expect(option(group("Alpha"), 0).getAttribute("aria-checked")).toBe("true");
  expect(option(group("Beta"), 2).getAttribute("aria-checked")).toBe("true");
  expect(host.textContent).toContain("voicePreferences.enabledGroups:1");

  act(() => option(group("Alpha"), 1).click());
  expect(change).toHaveBeenCalledWith({ groups: { g_b: "all_chat", g_a: "to_user" } });
});

it("saves the viewed-message switch", () => {
  const change = render("notifications");
  const toggle = host.querySelector<HTMLInputElement>('input[role="switch"]')!;
  expect(toggle.checked).toBe(true);
  act(() => toggle.click());
  expect(change).toHaveBeenCalledWith({ suppress_viewed: false });
});

it("filters the Group list and explains an empty match", () => {
  render("notifications");
  const search = host.querySelector<HTMLInputElement>('input[type="search"]')!;
  act(() => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    setter.call(search, "bet");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(group("Beta")).not.toBeNull();
  expect(host.querySelector('[role="radiogroup"][aria-label="Alpha"]')).toBeNull();
  act(() => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    setter.call(search, "zzz");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(host.textContent).toContain("voicePreferences.noMatchingGroups");
});

it("saves response verbosity and style from their option groups", () => {
  const change = render("audio");
  act(() => option(group("voicePreferences.verbosity"), 2).click());
  expect(change).toHaveBeenCalledWith({ verbosity: "detailed" });
  act(() => option(group("voicePreferences.style"), 1).click());
  expect(change).toHaveBeenCalledWith({ style: "direct" });
});

it("shows a failed first load instead of loading forever", () => {
  for (const section of ["audio", "notifications"] as const) {
    act(() =>
      root.render(
        <CodexVoicePreferenceFields
          preferences={
            { value: null, saving: false, saved: false, error: "boom", change: vi.fn() } as never
          }
          section={section}
        />,
      ),
    );
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("boom");
    expect(host.textContent).not.toContain("voicePreferences.loading");
  }
});
