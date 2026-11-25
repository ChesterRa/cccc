import { act } from "react";
import { expect } from "vite-plus/test";

export function sessionPickerActions(translate: (key: string) => string) {
  async function click(element: Element | null) {
    expect(element).not.toBeNull();
    expect(element?.closest("[hidden]")).toBeNull();
    await act(async () => {
      (element as HTMLElement).click();
    });
  }
  function button(label: string) {
    const buttons = [...document.querySelectorAll("button")];
    return buttons.find(
      (b) =>
        !b.closest("[hidden]") &&
        (b.textContent === label || b.getAttribute("aria-label") === label),
    )!;
  }
  function row(title: string) {
    return Array.from(document.body.querySelectorAll("label")).find((label) =>
      label.textContent?.includes(title),
    )!;
  }
  function checkbox(title: string) {
    return row(title).querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  }
  function foreman() {
    return document.querySelector<HTMLButtonElement>(
      `section[aria-label="${translate("localSessions.newGroupDefaults")}"] button[aria-haspopup="menu"]`,
    );
  }
  async function chooseForeman(sourceId: string) {
    await click(foreman());
    await click(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
        (option) => option.dataset.value === sourceId,
      )!,
    );
  }
  /** Switches the list between this computer's sessions and other computers'. */
  async function show(view: "local" | "other") {
    const label = translate(
      view === "local" ? "localSessions.thisDevice" : "localSessions.otherComputers",
    );
    await click(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="radio"]')).find((option) =>
        option.textContent?.startsWith(label),
      )!,
    );
  }
  return { click, button, row, checkbox, foreman, chooseForeman, show };
}
