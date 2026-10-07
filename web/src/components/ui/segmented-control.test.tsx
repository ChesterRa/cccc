// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SegmentedControl } from "./segmented-control";

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const OPTIONS = [
  { value: "off", label: "Off" },
  { value: "to_user", label: "To me" },
  { value: "all_chat", label: "All" },
] as const;
type Scope = (typeof OPTIONS)[number]["value"];

function Harness({ onChange, disabled }: { onChange: (v: Scope) => void; disabled?: boolean }) {
  const [value, setValue] = useState<Scope>("off");
  return (
    <SegmentedControl
      label="Scope"
      value={value}
      options={OPTIONS}
      disabled={disabled}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}
const radios = () => [...host.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
const checked = () => radios().map((r) => r.getAttribute("aria-checked"));

it("exposes one checked radio and reports a click on another option", () => {
  const onChange = vi.fn();
  act(() => root.render(<Harness onChange={onChange} />));
  expect(host.querySelector('[role="radiogroup"]')?.getAttribute("aria-label")).toBe("Scope");
  expect(checked()).toEqual(["true", "false", "false"]);
  // Only the checked radio is in the tab order.
  expect(radios().map((r) => r.tabIndex)).toEqual([0, -1, -1]);

  act(() => radios()[1].click());
  expect(onChange).toHaveBeenCalledWith("to_user");
  expect(checked()).toEqual(["false", "true", "false"]);

  // Clicking the current option is not a change.
  act(() => radios()[1].click());
  expect(onChange).toHaveBeenCalledTimes(1);
});

it("moves the selection and focus with arrow keys, wrapping at the ends", () => {
  const onChange = vi.fn();
  act(() => root.render(<Harness onChange={onChange} />));
  const press = (key: string) =>
    act(() => {
      (document.activeElement ?? radios()[0]).dispatchEvent(
        new KeyboardEvent("keydown", { key, bubbles: true }),
      );
    });
  radios()[0].focus();
  press("ArrowRight");
  expect(onChange).toHaveBeenLastCalledWith("to_user");
  expect(document.activeElement).toBe(radios()[1]);
  press("ArrowLeft");
  press("ArrowLeft");
  expect(onChange).toHaveBeenLastCalledWith("all_chat");
  expect(document.activeElement).toBe(radios()[2]);
});

it("does not change while disabled", () => {
  const onChange = vi.fn();
  act(() => root.render(<Harness onChange={onChange} disabled />));
  expect(radios().every((r) => r.disabled)).toBe(true);
  act(() => radios()[2].click());
  expect(onChange).not.toHaveBeenCalled();
  expect(checked()).toEqual(["true", "false", "false"]);
});
