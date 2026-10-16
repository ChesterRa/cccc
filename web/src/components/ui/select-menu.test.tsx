// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SelectMenu } from "./select-menu";

const options = Array.from({ length: 40 }, (_, index) => ({
  value: `actor-${index}`,
  label: `Actor ${index}`,
}));

let host: HTMLDivElement;
let root: Root;

function Harness() {
  const [value, setValue] = useState(options[0].value);
  return <SelectMenu value={value} options={options} onChange={setValue} ariaLabel="Sender" />;
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Harness />));
});

afterEach(() => {
  act(() => root.unmount());
  act(() => vi.runOnlyPendingTimers());
  vi.useRealTimers();
  host.remove();
});

it("preserves focus in a reopened menu when the preceding close finishes later", () => {
  const trigger = host.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!;
  act(() => trigger.click());
  const last = document.querySelector<HTMLButtonElement>('[role="menuitemradio"]:last-child')!;
  act(() => last.click());
  expect(trigger.dataset.value).toBe("actor-39");

  // Radix defers unmount autofocus. Reopen before that callback is delivered.
  act(() => trigger.click());
  const selected = document.querySelector<HTMLButtonElement>('[aria-checked="true"]')!;
  expect(document.activeElement).toBe(selected);
  act(() => vi.advanceTimersByTime(0));
  expect(document.activeElement).toBe(selected);

  act(() => selected.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })));
  expect((document.activeElement as HTMLElement).dataset.value).toBe("actor-0");
});
