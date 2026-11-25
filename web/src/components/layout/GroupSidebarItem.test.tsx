// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";
import { GroupSidebar } from "./GroupSidebar";
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
    // The row starts with the name; the status symbol sits at the row end.
    const row = host.querySelector('[role="button"]')!;
    expect(row.firstElementChild?.textContent).toBe("Alpha");
    expect(row.lastElementChild?.classList.contains("lcd-group-symbol")).toBe(true);

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

  // Narrow headers fold the "New" label first (at 13rem) so the plain brand name stays
  // visible down to 10.5rem (covering the 200 px sidebar at 100% text). Below 10.5rem a
  // plain name gives way to the logo, while a name that opens the instance card stays
  // reachable as its only entry.
  it("keeps the name that opens the instance details in a narrow header", async () => {
    const narrowHide = "@max-[10.5rem]/brand:hidden";
    await act(async () => root.render(<SidebarBrandName productName="CCCC" />));
    expect(host.firstElementChild?.classList.contains(narrowHide)).toBe(true);
    await act(async () =>
      root.render(<SidebarBrandName productName="CCCC" instanceLabel={name} />),
    );
    expect(host.querySelector("[data-brand-name]")?.tagName).toBe("BUTTON");
    expect(host.firstElementChild?.classList.contains(narrowHide)).toBe(false);
  });

  it("folds the New button label at 13rem before hiding a plain brand name at 10.5rem", async () => {
    const noop = () => {};
    await act(async () =>
      root.render(
        <GroupSidebar
          orderedGroups={[{ group_id: "g_1", title: "Alpha" } as GroupMeta]}
          archivedGroupIds={[]}
          selectedGroupId="g_1"
          isOpen
          isCollapsed={false}
          sidebarWidth={200}
          isDark={false}
          onSelectGroup={noop}
          onCreateGroup={noop}
          onClose={noop}
          onToggleCollapse={noop}
          onResizeWidth={noop}
          onReorderSection={noop}
          onArchiveGroup={noop}
          onRestoreGroup={noop}
        />,
      ),
    );
    const brandContainer = host.querySelector('[class*="@container/brand"]');
    expect(brandContainer).not.toBeNull();
    const brandName = host.querySelector<HTMLElement>("[data-brand-name]");
    expect(brandName?.parentElement?.classList.contains("@max-[10.5rem]/brand:hidden")).toBe(true);
    expect(brandName?.parentElement?.parentElement?.className).toContain(
      "@max-[13rem]/brand:gap-2",
    );
    const newButton = host.querySelector<HTMLButtonElement>('button[aria-label="createNewGroup"]');
    expect(newButton).not.toBeNull();
    expect(newButton?.getAttribute("title")).toBe("createNewGroup");
    const newLabel = newButton?.querySelector("span");
    expect(newLabel?.textContent).toBe("newGroup");
    expect(newLabel?.classList.contains("@max-[13rem]/brand:hidden")).toBe(true);
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
