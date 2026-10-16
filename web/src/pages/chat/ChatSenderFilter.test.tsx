// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../types";
import { ChatSenderFilter } from "./ChatSenderFilter";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { count?: number }) => (args?.count ? `${key} ${args.count}` : key),
  }),
}));
const actors = [
  { id: "leader", title: "Coordinator", role: "foreman" },
  { id: "peer", title: "Worker", role: "peer" },
] as Actor[];
let host: HTMLDivElement;
let root: Root;
function Harness({ roster = actors, initial = [] }: { roster?: Actor[]; initial?: string[] }) {
  const [value, setValue] = useState(initial);
  return <ChatSenderFilter value={value} actors={roster} onChange={setValue} />;
}
const trigger = () => host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
const choices = () => [...document.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
const checked = () =>
  choices()
    .filter((input) => input.checked)
    .map((input) => input.value);
const choose = (id: string) =>
  act(() =>
    choices()
      .find((input) => input.value === id)!
      .click(),
  );
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  if (vi.isFakeTimers()) act(() => vi.runOnlyPendingTimers());
  vi.useRealTimers();
  host.remove();
});

it("lists each sender once with the foreman role on the person's row", () => {
  act(() => root.render(<Harness />));
  act(() => trigger().click());
  expect(choices().map((input) => input.value)).toEqual(["user", "leader", "peer"]);
  expect(choices()[1].closest("label")?.textContent).toContain("senderForeman");
  expect(choices()[2].closest("label")?.textContent).not.toContain("senderForeman");
  expect(checked()).toEqual([]);
});

it("keeps the popup open while toggling several senders and shows the count", () => {
  act(() => root.render(<Harness />));
  act(() => trigger().click());
  choose("leader");
  expect(trigger().textContent).toContain("Coordinator");
  choose("peer");
  expect(checked()).toEqual(["leader", "peer"]);
  expect(trigger().textContent).toContain("senderCount 2");
  choose("leader");
  expect(checked()).toEqual(["peer"]);
  choose("peer");
  expect(checked()).toEqual([]);
  expect(trigger().textContent).toContain("senderLabel");
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
});

it("shows all without closing the popup or leaving a false all-checkbox", () => {
  act(() => root.render(<Harness initial={["leader", "peer"]} />));
  act(() => trigger().click());
  const reset = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
    (button) => button.textContent === "senderShowAll",
  )!;
  act(() => reset.click());
  expect(checked()).toEqual([]);
  expect(reset.disabled).toBe(true);
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(host.querySelector("button[title]")).toBeNull();
});

it("keeps selected identities fixed when the foreman changes and when an Actor is removed", () => {
  act(() => root.render(<Harness initial={["leader"]} />));
  act(() => trigger().click());
  act(() =>
    root.render(<Harness initial={["leader"]} roster={[{ ...actors[1], role: "foreman" }]} />),
  );
  expect(checked()).toEqual(["leader"]);
  expect(
    choices()
      .find((input) => input.value === "peer")
      ?.closest("label")?.textContent,
  ).toContain("senderForeman");
  choose("leader");
  expect(checked()).toEqual([]);
});

it("does not let an old close callback steal focus after reopening", () => {
  vi.useFakeTimers();
  act(() => root.render(<Harness initial={["peer"]} />));
  act(() => trigger().click());
  act(() => trigger().click());
  act(() => trigger().click());
  const selected = choices().find((input) => input.checked)!;
  expect(document.activeElement).toBe(selected);
  act(() => vi.advanceTimersByTime(0));
  expect(document.activeElement).toBe(selected);
  act(() => selected.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })));
  expect(document.activeElement).toBe(choices()[0]);
});
