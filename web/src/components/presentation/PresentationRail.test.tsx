// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vite-plus/test";
import { PresentationRail } from "./PresentationRail";
import type { GroupPresentation } from "../../types";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("../../services/api", () => ({
  getPresentationAssetUrl: (_group: string, slot: string) => `/assets/${slot}`,
  refreshAuthTokenInUrl: (url: string) => url,
}));
const presentation = {
  v: 1,
  slots: [
    {
      slot_id: "slot-1",
      index: 1,
      card: { card_type: "image", title: "Diagram", published_at: "one", content: {} },
    },
    {
      slot_id: "slot-2",
      index: 2,
      card: {
        card_type: "markdown",
        title: "Notes",
        published_at: "two",
        content: { mode: "inline", markdown: "# Findings\n\nActual useful content" },
      },
    },
    {
      slot_id: "slot-3",
      index: 3,
      card: {
        card_type: "table",
        title: "Results",
        published_at: "three",
        content: { table: { columns: ["Name", "Value"], rows: [["Latency", "42ms"]] } },
      },
    },
    {
      slot_id: "slot-4",
      index: 4,
      card: {
        card_type: "web_preview",
        title: "Website",
        published_at: "four",
        content: { url: "https://example.test/product" },
      },
    },
  ],
} as GroupPresentation;
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
const open = vi.fn();
async function render(
  compact = false,
  value = presentation,
  readOnly = false,
  toggle?: () => void,
) {
  await act(async () =>
    root.render(
      <PresentationRail
        groupId="g"
        presentation={value}
        isDark={false}
        compact={compact}
        readOnly={readOnly}
        attentionSlots={{ "slot-2": true }}
        onOpenSlot={open}
        onToggleCompact={toggle}
        onPinSlot={vi.fn()}
      />,
    ),
  );
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  open.mockClear();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
it("shows lightweight identifiable entries without mounting interactive viewers or opening slots", async () => {
  await render();
  expect(host.querySelector("img")?.getAttribute("src")).toBe("/assets/slot-1");
  expect(host.textContent).toContain("Notes");
  expect(host.textContent).toContain("Results");
  expect(host.textContent).toContain("example.test");
  expect(host.querySelector("iframe,canvas")).toBeNull();
  expect(open).not.toHaveBeenCalled();
  expect(host.querySelector("img")?.getAttribute("loading")).toBe("lazy");
});
it("keeps all four slots and their update indication directly usable when compact", async () => {
  await render(true);
  const slots = host.querySelectorAll<HTMLButtonElement>(
    'button[aria-label^="presentationOpenSlot"]',
  );
  expect(slots).toHaveLength(4);
  await act(async () => slots[1].click());
  expect(open).toHaveBeenCalledWith("slot-2");
  expect(host.querySelector(".ring-2")).not.toBeNull();
});
it("keeps read-only empty slots inert and recovers an image after its publication changes", async () => {
  await render();
  await act(async () => host.querySelector("img")!.dispatchEvent(new Event("error")));
  expect(host.querySelector("img")).toBeNull();
  await render(false, {
    ...presentation,
    slots: presentation.slots.map((slot, index) =>
      index === 0 ? { ...slot, card: { ...slot.card!, published_at: "new" } } : slot,
    ),
  });
  expect(host.querySelector("img")).not.toBeNull();
  await render(false, { v: 1, slots: [] } as unknown as GroupPresentation, true);
  expect([...host.querySelectorAll("button")].every((b) => b.disabled)).toBe(true);
});

it("retains keyboard focus on the density control after its layout changes", async () => {
  const toggle = vi.fn();
  await render(false, presentation, false, toggle);
  const button = host.querySelector<HTMLButtonElement>('button[title="presentationCompactSlots"]')!;
  button.focus();
  await act(async () => button.click());
  expect(toggle).toHaveBeenCalledOnce();
  await render(true, presentation, false, toggle);
  expect(document.activeElement).toBe(
    host.querySelector('button[title="presentationExpandSlots"]'),
  );
});
