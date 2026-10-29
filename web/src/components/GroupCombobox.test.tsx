// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { GroupCombobox } from "./GroupCombobox";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function Fixture({
  searchable = false,
  emptyFirst = false,
}: {
  searchable?: boolean;
  emptyFirst?: boolean;
}) {
  const [value, setValue] = useState(emptyFirst ? "" : "first");
  return (
    <GroupCombobox
      items={[
        { value: emptyFirst ? "" : "first", label: "First" },
        { value: "second-key", label: "Second" },
      ]}
      value={value}
      onChange={setValue}
      placeholder="Choose"
      searchPlaceholder="Search"
      emptyText="No matches"
      ariaLabel="Test choice"
      searchable={searchable}
    />
  );
}

async function open(searchable = false, emptyFirst = false) {
  await act(async () => root.render(<Fixture searchable={searchable} emptyFirst={emptyFirst} />));
  const trigger = host.querySelector<HTMLButtonElement>('button[role="combobox"]')!;
  trigger.focus();
  await act(async () => trigger.click());
  // Radix moves focus after its popover has mounted.
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  return trigger;
}

it("lets a non-searchable list be operated with arrow keys and Enter", async () => {
  const trigger = await open();
  const list = document.querySelector<HTMLElement>('[role="listbox"]')!;
  expect(document.activeElement).toBe(list);
  expect(list.getAttribute("aria-label")).toBe("Test choice");
  await act(async () => {
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  });
  const active = document.querySelector<HTMLElement>('[cmdk-item][aria-selected="true"]')!;
  expect(active.textContent).toBe("Second");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(list.getAttribute("aria-activedescendant")).toBe(active.id);
  await act(async () => {
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  expect(trigger.textContent).toBe("Second");
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(document.activeElement).toBe(trigger);
});

it("can move past an empty-valued option using the keyboard", async () => {
  const trigger = await open(false, true);
  const list = document.querySelector<HTMLElement>('[role="listbox"]')!;
  await act(async () => {
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  });
  expect(document.querySelector('[cmdk-item][aria-selected="true"]')?.textContent).toBe("Second");
  await act(async () => {
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  expect(trigger.textContent).toBe("Second");
  await act(async () => trigger.click());
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  const reopened = document.querySelector<HTMLElement>('[role="listbox"]')!;
  await act(async () => {
    reopened.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    reopened.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  expect(trigger.textContent).toBe("First");
});

it("keeps values that differ only by whitespace distinct", async () => {
  const onChange = vi.fn();
  await act(async () =>
    root.render(
      <GroupCombobox
        items={[
          { value: "tag", label: "Plain" },
          { value: "tag ", label: "Trailing space" },
        ]}
        value="tag"
        onChange={onChange}
        placeholder="Choose"
        searchPlaceholder="Search"
        emptyText="No matches"
        ariaLabel="Test choice"
        searchable={false}
      />,
    ),
  );
  const trigger = host.querySelector<HTMLButtonElement>('button[role="combobox"]')!;
  await act(async () => trigger.click());
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  const list = document.querySelector<HTMLElement>('[role="listbox"]')!;
  await act(async () => {
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  });
  expect(document.querySelector('[cmdk-item][aria-selected="true"]')?.textContent).toBe(
    "Trailing space",
  );
  await act(async () => {
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  expect(onChange).toHaveBeenCalledWith("tag ");
});

it("keeps the searchable input focused and lets the mouse select an item", async () => {
  const trigger = await open(true);
  const input = document.querySelector<HTMLInputElement>("[cmdk-input]")!;
  expect(document.activeElement).toBe(input);
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(
      input,
      "second-key",
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(
    [...document.querySelectorAll<HTMLElement>("[cmdk-item]")].map((item) => item.textContent),
  ).toEqual(["Second"]);
  const second = [...document.querySelectorAll<HTMLElement>("[cmdk-item]")].find(
    (item) => item.textContent === "Second",
  )!;
  await act(async () => second.click());
  expect(trigger.textContent).toBe("Second");
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(document.activeElement).toBe(trigger);
});

it("does not match searches against the internal option marker", async () => {
  await open(true);
  const input = document.querySelector<HTMLInputElement>("[cmdk-input]")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, "=");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(document.querySelectorAll("[cmdk-item]")).toHaveLength(0);
  expect(document.body.textContent).toContain("No matches");
});

it("still accepts a mouse choice in a non-searchable list", async () => {
  const trigger = await open();
  const second = [...document.querySelectorAll<HTMLElement>("[cmdk-item]")].find(
    (item) => item.textContent === "Second",
  )!;
  await act(async () => second.click());
  expect(trigger.textContent).toBe("Second");
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
});
