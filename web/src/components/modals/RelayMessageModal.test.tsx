// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../../services/api";
import type { GroupMeta, LedgerEvent } from "../../types";
import { RelayMessageModal } from "./RelayMessageModal";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("../../services/api", () => ({
  fetchActors: vi.fn(async (groupId: string) => ({
    ok: true,
    result: {
      actors:
        groupId === "other" ? [{ id: "destpeer" }, { id: "otherpeer" }] : [{ id: "destpeer" }],
    },
  })),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const source: LedgerEvent = { id: "source-event", by: "user", data: { text: "Original message" } };
const groups: GroupMeta[] = [{ group_id: "source" }, { group_id: "destination" }];

beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(destinations: GroupMeta[], onSubmit = vi.fn()) {
  await act(async () => {
    root.render(
      <RelayMessageModal
        isOpen
        busy={false}
        srcGroupId="source"
        srcEvent={source}
        groups={destinations}
        onCancel={() => {}}
        onSubmit={onSubmit}
      />,
    );
  });
  return onSubmit;
}

function button(text: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find(
    (element) => element.textContent === text,
  );
  if (!found) throw new Error(`Missing button ${text}`);
  return found;
}

async function selectDestination(label: string) {
  const trigger = host.querySelector<HTMLButtonElement>('button[role="combobox"]')!;
  await act(async () => trigger.click());
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
    (element) => element.textContent === label,
  );
  if (!option) throw new Error(`Missing destination ${label}`);
  await act(async () => option.click());
}

it("allows multiple recipients to be toggled and submits the chosen recipients and note", async () => {
  const onSubmit = await render(groups);
  expect(button("@all").getAttribute("aria-pressed")).toBe("true");
  expect(button("@foreman").getAttribute("aria-pressed")).toBe("false");
  await act(async () => button("@foreman").click());
  expect(button("@all").getAttribute("aria-pressed")).toBe("true");
  expect(button("@foreman").getAttribute("aria-pressed")).toBe("true");
  await act(async () => button("@all").click());
  const note = host.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
      note,
      "context",
    );
    note.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledOnce();
  expect(onSubmit).toHaveBeenCalledWith("destination", ["@foreman"], "context");
});

it("explains the absence of another group and prevents submission", async () => {
  const onSubmit = await render([{ group_id: "source" }]);
  expect(host.textContent).toContain("relay.noOtherGroups");
  expect(host.querySelector<HTMLButtonElement>('button[role="combobox"]')?.disabled).toBe(true);
  expect(button("relay.relay").disabled).toBe(true);
  await act(async () => button("relay.relay").click());
  expect(onSubmit).not.toHaveBeenCalled();
});

it("keeps loaded recipients when the current destination is selected again", async () => {
  await render(groups);
  expect(button("destpeer")).toBeDefined();
  expect(api.fetchActors).toHaveBeenCalledTimes(1);
  await selectDestination("destination");
  expect(button("destpeer")).toBeDefined();
  expect(api.fetchActors).toHaveBeenCalledTimes(1);
});

it("requires a new recipient choice when a personal selection crosses groups", async () => {
  const onSubmit = await render([...groups, { group_id: "other" }]);
  await act(async () => button("@all").click());
  await act(async () => button("destpeer").click());
  await selectDestination("other");
  expect(host.textContent).toContain("relay.reselectRecipients");
  expect(button("relay.relay").disabled).toBe(true);
  // The same ID in the new group is a different selection and must not be inherited.
  expect(button("destpeer").getAttribute("aria-pressed")).toBe("false");
  await act(async () => button("relay.relay").click());
  expect(onSubmit).not.toHaveBeenCalled();
  await act(async () => button("otherpeer").click());
  expect(host.textContent).not.toContain("relay.reselectRecipients");
  expect(button("relay.relay").disabled).toBe(false);
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledWith("other", ["otherpeer"], "");
});

it("keeps group selectors on group change and lets Reset explicitly choose the group default", async () => {
  const onSubmit = await render([...groups, { group_id: "other" }]);
  await selectDestination("other");
  expect(button("@all").getAttribute("aria-pressed")).toBe("true");
  expect(host.textContent).not.toContain("relay.reselectRecipients");
  await act(async () => button("@all").click());
  await act(async () => button("destpeer").click());
  await selectDestination("destination");
  expect(button("relay.relay").disabled).toBe(true);
  await act(async () => button("common:reset").click());
  expect(host.textContent).toContain("relay.selectedDefault");
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledWith("destination", [], "");
});

it("clears mixed personal and group selections and keeps review pending across A-B-A", async () => {
  const onSubmit = await render([...groups, { group_id: "other" }]);
  await act(async () => button("@all").click());
  await act(async () => button("@foreman").click());
  await act(async () => button("destpeer").click());
  await selectDestination("other");
  expect(button("@foreman").getAttribute("aria-pressed")).toBe("false");
  expect(button("destpeer").getAttribute("aria-pressed")).toBe("false");
  expect(host.textContent).toContain("relay.reselectRecipients");
  expect(host.textContent).not.toContain("relay.selectedDefault");
  expect(button("relay.relay").disabled).toBe(true);
  await selectDestination("destination");
  expect(button("destpeer").getAttribute("aria-pressed")).toBe("false");
  expect(button("relay.relay").disabled).toBe(true);
  await act(async () => button("@foreman").click());
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledWith("destination", ["@foreman"], "");
});

it("retains an explicitly selected group role when there are no personal recipients", async () => {
  const onSubmit = await render([...groups, { group_id: "other" }]);
  await act(async () => button("@all").click());
  await act(async () => button("@foreman").click());
  await selectDestination("other");
  expect(button("@foreman").getAttribute("aria-pressed")).toBe("true");
  expect(host.textContent).not.toContain("relay.reselectRecipients");
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledWith("other", ["@foreman"], "");
});

it("keeps recipient review pending through further group changes until an explicit choice", async () => {
  const onSubmit = await render([...groups, { group_id: "other" }, { group_id: "third" }]);
  await act(async () => button("@all").click());
  await act(async () => button("destpeer").click());
  await selectDestination("other");
  await selectDestination("other");
  expect(button("relay.relay").disabled).toBe(true);
  await selectDestination("third");
  expect(host.textContent).toContain("relay.reselectRecipients");
  expect(host.textContent).not.toContain("relay.selectedDefault");
  expect(button("relay.relay").disabled).toBe(true);
  await act(async () => button("common:reset").click());
  expect(button("relay.relay").disabled).toBe(false);
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledWith("third", [], "");
});

it("keeps recipient review pending while the new group's actor lookup is slow or fails", async () => {
  const onSubmit = await render([...groups, { group_id: "other" }]);
  await act(async () => button("@all").click());
  await act(async () => button("destpeer").click());
  let finishLookup: (() => void) | undefined;
  vi.mocked(api.fetchActors).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishLookup = () =>
          resolve({ ok: false, error: { code: "unavailable", message: "fixture failure" } });
      }),
  );
  await selectDestination("other");
  expect(host.textContent).toContain("relay.reselectRecipients");
  expect(button("relay.relay").disabled).toBe(true);
  if (!finishLookup) throw new Error("Destination lookup did not start");
  await act(async () => finishLookup!());
  expect(host.textContent).toContain("relay.reselectRecipients");
  expect(button("relay.relay").disabled).toBe(true);
  await act(async () => button("relay.relay").click());
  expect(onSubmit).not.toHaveBeenCalled();
  await act(async () => button("@all").click());
  await act(async () => button("relay.relay").click());
  expect(onSubmit).toHaveBeenCalledWith("other", ["@all"], "");
});

it.each([
  { groupTop: 240, fits: true },
  { groupTop: 120, fits: false },
])(
  "keeps the destination visible when revealing the selected recipient (fits: $fits)",
  async ({ groupTop, fits }) => {
    vi.spyOn(window, "matchMedia").mockImplementation((query) => ({
      matches: query.includes("max-width"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }));
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(
      function (this: HTMLElement) {
        return this.matches(".min-h-0.flex-1.overflow-y-auto") ? 300 : 0;
      },
    );
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        const scroll = host.querySelector<HTMLElement>(".min-h-0.flex-1.overflow-y-auto");
        // The real modal opens at 95% scale; scrolling itself is in unscaled CSS pixels.
        const offset = (scroll?.scrollTop ?? 0) * 0.95;
        if (this === scroll) return new DOMRect(0, 100, 300, 285);
        if (this.matches('button[role="combobox"]'))
          return new DOMRect(10, groupTop - offset, 200, 40);
        if (this.textContent === "@all") return new DOMRect(10, 393 - offset, 55, 40);
        return new DOMRect();
      },
    );

    await render(groups);
    await act(async () => new Promise((resolve) => setTimeout(resolve, 80)));
    const scroll = host.querySelector<HTMLElement>(".min-h-0.flex-1.overflow-y-auto")!;
    const destination = host.querySelector<HTMLButtonElement>('button[role="combobox"]')!;
    expect(document.activeElement).toBe(destination);
    expect(destination.getBoundingClientRect().top).toBeGreaterThanOrEqual(100);
    if (fits) {
      expect(scroll.scrollTop).toBeGreaterThan(0);
      expect(button("@all").getBoundingClientRect().bottom).toBeLessThanOrEqual(385);
    } else {
      expect(scroll.scrollTop).toBe(0);
    }
  },
);
