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
    return buttons.find((b) => b.textContent === label || b.getAttribute("aria-label") === label)!;
  }
  function row(title: string) {
    return Array.from(document.body.querySelectorAll("label")).find((label) =>
      label.textContent?.includes(title),
    )!;
  }
  function checkbox(title: string) {
    return row(title).querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  }
  function target() {
    return document.querySelector<HTMLButtonElement>(
      `section[aria-label="${translate("localSessions.target")}"] button[aria-haspopup="menu"]`,
    )!;
  }
  function foreman() {
    return document.querySelector<HTMLButtonElement>(
      `section[aria-label="${translate("localSessions.newGroupDefaults")}"] button[aria-haspopup="menu"]`,
    );
  }
  async function chooseForeman(sourceId: string) {
    await reviewSelection();
    await click(foreman());
    await click(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
        (option) => option.dataset.value === sourceId,
      )!,
    );
  }
  async function chooseTarget(groupId: string) {
    await reviewSelection();
    await click(target());
    await click(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
        (option) => option.dataset.value === groupId,
      )!,
    );
  }
  function primary() {
    if (!target()) return button(translate("localSessions.continue"));
    return button(
      translate(target().dataset.value ? "localSessions.join" : "localSessions.create"),
    );
  }
  async function reviewSelection() {
    if (!target() && primary()) await click(primary());
  }
  async function submitSelection() {
    await reviewSelection();
    if (target()) await click(primary());
  }
  async function overview() {
    await click(button(translate("localSessions.backToSessions")));
  }
  return {
    click,
    button,
    row,
    checkbox,
    target,
    foreman,
    chooseForeman,
    chooseTarget,
    primary,
    reviewSelection,
    submitSelection,
    overview,
  };
}
