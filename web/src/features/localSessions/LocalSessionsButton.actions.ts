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
  function expectOverview(source: { title: string; cwd: string }) {
    expect(button(translate("localSessions.connect")).title).toBe(
      translate("localSessions.connect"),
    );
    expect(document.querySelector('[role="dialog"]')?.getAttribute("aria-labelledby")).toBe(
      "local-sessions-title",
    );
    expect(document.getElementById("local-sessions-title")?.textContent).toBe(
      translate("localSessions.overviewTitle"),
    );
    expect(document.body.textContent).toContain(source.title);
    expect(document.body.textContent).toContain(source.cwd);
    expect(row(source.title).closest("section")?.textContent).toContain(
      translate("localSessions.thisDevice"),
    );
    expect(row(source.title).textContent).toContain(translate("localSessions.ready"));
    expect(document.querySelector('[role="radiogroup"]')).toBeNull();
    expect(target()).toBeNull();
    expect(foreman()).toBeNull();
    expect(document.body.textContent).toContain("Selected sessions: 0 / 16");
    expect(primary().disabled).toBe(true);
  }
  function expectSelectedDetails(details: string[]) {
    const selected = document.querySelector(
      `section[aria-label="${translate("localSessions.selectedSessions")}"]`,
    );
    for (const detail of details) expect(selected?.textContent).toContain(detail);
  }
  async function chooseForeman(sourceId: string) {
    await chooseTarget("");
    await click(foreman());
    await click(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
        (option) => option.dataset.value === sourceId,
      )!,
    );
  }
  async function chooseTarget(groupId: string) {
    await reviewSelection();
    if (groupId === "") return click(button(translate("localSessions.newConversation")));
    await click(target());
    await click(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
        (option) => option.dataset.value === groupId,
      )!,
    );
  }
  function primary() {
    const labels = ["continue", "chooseTarget", "join", "create"].map((key) =>
      translate(`localSessions.${key}`),
    );
    return [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')]
      .reverse()
      .find((entry) => labels.includes(entry.textContent || ""))!;
  }
  const reviewing = () =>
    !!document.querySelector(`section[aria-label="${translate("localSessions.target")}"]`);
  async function reviewSelection() {
    if (!reviewing() && primary()) await click(primary()!);
  }
  async function submitSelection() {
    await reviewSelection();
    if (reviewing()) await click(primary()!);
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
    expectOverview,
    expectSelectedDetails,
    chooseForeman,
    chooseTarget,
    primary,
    reviewSelection,
    submitSelection,
    overview,
  };
}
