// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";
import { GroupSidebarSectionHeader } from "./GroupSidebarSectionHeader";
import { GroupSidebarItem } from "./GroupSidebarItem";
import { BRAND_CARD_CLOSE_DELAY_MS, SidebarBrandName } from "./SidebarBrandName";

describe("GroupSidebarItem mobile actions", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps the menu visible and isolates its action from group selection", async () => {
    const onMenuAction = vi.fn();
    const onSelect = vi.fn();
    await act(async () =>
      root.render(
        <GroupSidebarItem
          group={{ group_id: "g_alpha", title: "Alpha" } as GroupMeta}
          isActive
          isCollapsed={false}
          menuActionLabel="Archive"
          menuAriaLabel="Actions · Alpha"
          onMenuAction={onMenuAction}
          onSelect={onSelect}
        />,
      ),
    );

    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Alpha"]');
    expect(trigger).not.toBeNull();
    expect(trigger?.getAttribute("aria-haspopup")).toBe("menu");
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('[role="button"]')?.getAttribute("aria-current")).toBe("page");
    expect(host.querySelector('[aria-label^="Reorder"]')).toBeNull();
    // The row is the name alone: its run state is not drawn as a separate symbol.
    const row = host.querySelector('[role="button"]')!;
    expect(row.querySelector("[data-group-name]")?.textContent).toBe("Alpha");
    expect(row.querySelector(".lcd-group-symbol")).toBeNull();

    await act(async () => trigger?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    const action = document.querySelector<HTMLButtonElement>('[role="menuitem"]');
    expect(action?.textContent).toBe("Archive");

    await act(async () => action?.click());
    expect(onMenuAction).toHaveBeenCalledOnce();
    expect(onSelect).not.toHaveBeenCalled();
  });
  it("names the Group list for screen readers only and draws no empty row", async () => {
    await act(async () => root.render(<GroupSidebarSectionHeader />));
    // Only the hidden name: no row of padding above the list.
    expect([...host.children].map((child) => child.className)).toEqual(["sr-only"]);
    expect(host.textContent).toBe("workingGroups");
    await act(async () =>
      root.render(<GroupSidebarSectionHeader action={<button type="button">Connect</button>} />),
    );
    const named = [...host.querySelectorAll("*")].filter((node) =>
      [...node.childNodes].some(
        (child) => child.nodeType === 3 && child.textContent === "workingGroups",
      ),
    );
    expect(named.map((node) => node.className)).toEqual(["sr-only"]);
    expect(host.querySelector("button")?.textContent).toBe("Connect");
  });
});

describe("GroupSidebarItem run state", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  const renderRow = async (runtime: Record<string, unknown>, isActive = false) => {
    await act(async () =>
      root.render(
        <GroupSidebarItem
          group={{ group_id: "g_alpha", title: "Alpha", runtime_status: runtime } as GroupMeta}
          isActive={isActive}
          isCollapsed={false}
          onSelect={vi.fn()}
        />,
      ),
    );
    const name = host.querySelector<HTMLElement>("[data-group-name]")!;
    const state = host.querySelector<HTMLElement>("[data-group-status]")!;
    return { name, state };
  };

  it("shows the state through the name's tone and names it in text, without a symbol", async () => {
    const cases: [Record<string, unknown>, string, string][] = [
      [{ lifecycle_state: "active", runtime_running: true }, "run", "--color-text-primary"],
      [{ lifecycle_state: "idle", runtime_running: true }, "idle", "--color-text-primary"],
      [{ lifecycle_state: "paused", runtime_running: true }, "paused", "--color-text-secondary"],
      [{ lifecycle_state: "stopped", runtime_running: false }, "stop", "--color-text-muted"],
      [{ lifecycle_state: "active", runtime_running: false }, "offline", "--color-text-muted"],
    ];
    for (const [runtime, key, tone] of cases) {
      const { name, state } = await renderRow(runtime);
      expect(state.dataset.groupStatus).toBe(key);
      expect(name.className).toContain(tone);
      // The state is named for assistive technology and in the tooltip, not by tone alone.
      expect(state.querySelector(".sr-only")?.textContent).toBeTruthy();
      expect(name.title).toMatch(/^Alpha · ./);
      expect(host.querySelector(".lcd-group-symbol")).toBeNull();
      expect(state.textContent).not.toContain("?");
    }
  });

  it("keeps a mark only for an unknown state, which needs attention", async () => {
    const { state } = await renderRow({ lifecycle_state: "active", available: false });
    expect(state.dataset.groupStatus).toBe("unknown");
    expect(state.querySelector('[aria-hidden="true"]')?.textContent).toBe("?");
  });

  it("draws the open Group's name in the active style whatever its state", async () => {
    const { name } = await renderRow({ lifecycle_state: "stopped", runtime_running: false }, true);
    expect(name.className).toContain("font-semibold");
    expect(name.className).not.toContain("--color-text-muted");
  });
});

describe("SidebarBrandName", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  const name = "workstation-with-a-long-name.local";
  // A closing card stays mounted for its fade-out, so only an open one counts.
  const details = () => document.querySelector('[data-brand-instance-info][data-state="open"]');
  // React derives enter and leave from over and out, as a browser reports them.
  const pointer = (type: string, pointerType: string, relatedTarget: Element | null = null) =>
    new PointerEvent(type, { bubbles: true, pointerType, relatedTarget });

  it("is plain text when there are no instance details", async () => {
    await act(async () => root.render(<SidebarBrandName productName="CCCC" />));
    expect(host.textContent).toBe("CCCC");
    expect(host.querySelector("button")).toBeNull();
  });

  it("shows the complete instance details while the pointer is over the name", async () => {
    await act(async () =>
      root.render(
        <SidebarBrandName
          productName="CCCC"
          instanceLabel={name}
          instanceOrigin="https://instance.example"
        />,
      ),
    );
    const trigger = host.querySelector<HTMLButtonElement>("[data-brand-name]")!;
    expect(trigger.textContent).toBe("CCCC");
    expect(details()).toBeNull();
    await act(async () => trigger.dispatchEvent(pointer("pointerover", "mouse", document.body)));
    expect(details()?.textContent).toContain(name);
    expect(details()?.textContent).toContain("https://instance.example");
    // Moving onto the card keeps it open, so its copy key can be reached.
    vi.useFakeTimers();
    try {
      const card = details() as HTMLElement;
      await act(async () => trigger.dispatchEvent(pointer("pointerout", "mouse", card)));
      await act(async () => card.dispatchEvent(pointer("pointerover", "mouse", trigger)));
      await act(async () => vi.advanceTimersByTime(BRAND_CARD_CLOSE_DELAY_MS * 2));
      expect(details()).not.toBeNull();
      // Leaving both closes it after the short delay.
      await act(async () => card.dispatchEvent(pointer("pointerout", "mouse", document.body)));
      expect(details()).not.toBeNull();
      await act(async () => vi.advanceTimersByTime(BRAND_CARD_CLOSE_DELAY_MS));
      expect(details()).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("copies the public address from the card", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    await act(async () =>
      root.render(
        <SidebarBrandName
          productName="CCCC"
          instanceLabel={name}
          instanceOrigin="https://instance.example"
        />,
      ),
    );
    await act(async () => host.querySelector<HTMLButtonElement>("[data-brand-name]")!.click());
    const copy = document.querySelector<HTMLButtonElement>("[data-brand-copy-origin]")!;
    expect(copy.getAttribute("aria-label")).toBe("copy");
    await act(async () => copy.click());
    expect(writeText).toHaveBeenCalledWith("https://instance.example");
    expect(copy.getAttribute("aria-label")).toBe("copied");
  });

  it("offers no copy key without a public address", async () => {
    await act(async () =>
      root.render(<SidebarBrandName productName="CCCC" instanceLabel={name} />),
    );
    await act(async () => host.querySelector<HTMLButtonElement>("[data-brand-name]")!.click());
    expect(details()).not.toBeNull();
    expect(document.querySelector("[data-brand-copy-origin]")).toBeNull();
  });

  it("toggles on a tap or Enter, where there is no hover", async () => {
    await act(async () =>
      root.render(<SidebarBrandName productName="CCCC" instanceLabel={name} />),
    );
    const trigger = host.querySelector<HTMLButtonElement>("[data-brand-name]")!;
    // A plain button, so Enter and Space activate it like a tap.
    expect(trigger.tagName).toBe("BUTTON");
    expect(trigger.getAttribute("aria-label")).toBe("CCCC · sidebarInstanceInfo");
    await act(async () => trigger.click());
    // With no public address the card says the instance has no remote access.
    expect(details()?.textContent).toContain("connect.noRemoteAccess");
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    await act(async () => trigger.click());
    expect(details()).toBeNull();
  });
});
