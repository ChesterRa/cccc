// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { GroupRunControl } from "./GroupRunControl";
import { GroupSidebarItem } from "./GroupSidebarItem";
import type { GroupRunControls } from "../../utils/groupControls";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
const group = { group_id: "background", title: "Background", running: true };
async function mount(element: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(element));
}
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
});

describe("Group status control", () => {
  it("isolates pointer, mouse, touch and keyboard presses from sortable row handlers", async () => {
    const parent = vi.fn(),
      run = vi.fn(async () => {});
    await mount(
      <div
        onPointerDown={parent}
        onMouseDown={parent}
        onTouchStart={parent}
        onClick={parent}
        onKeyDown={parent}
      >
        <GroupRunControl group={group} controls={{ pending: null, run }} compact />
      </div>,
    );
    const button = host.querySelector("button")!;
    await act(async () => {
      for (const type of ["pointerdown", "mousedown", "touchstart"])
        button.dispatchEvent(new Event(type, { bubbles: true }));
      button.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    });
    expect(parent).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
    expect(document.querySelector('[role="menu"]')).not.toBeNull();
    await act(async () => document.querySelector<HTMLButtonElement>('[role="menuitem"]')!.click());
    expect(run).toHaveBeenCalledExactlyOnceWith("background", "pause");
    expect(parent).not.toHaveBeenCalled();
  });

  it("shares pending feedback without spinning on unrelated Groups or allowing another action", async () => {
    const controls: GroupRunControls = {
      pending: { groupId: "background", action: "stop" },
      run: vi.fn(async () => {}),
    };
    await mount(
      <>
        <GroupRunControl group={group} controls={controls} />
        <GroupRunControl group={{ ...group, group_id: "other" }} controls={controls} />
      </>,
    );
    expect(
      host.querySelector('[data-group-run-control="background"]')?.getAttribute("aria-busy"),
    ).toBe("true");
    const other = host.querySelector<HTMLButtonElement>('[data-group-run-control="other"]')!;
    expect(other.getAttribute("aria-busy")).toBe("false");
    await act(async () => other.click());
    expect(
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).every(
        (b) => b.disabled,
      ),
    ).toBe(true);
  });

  it("keeps read-only and collapsed navigation passive", async () => {
    const onSelect = vi.fn();
    await mount(
      <GroupSidebarItem
        group={group}
        isActive={false}
        isCollapsed
        onSelect={onSelect}
        groupRunControls={{ pending: null, run: vi.fn(async () => {}) }}
      />,
    );
    expect(host.querySelectorAll("button")).toHaveLength(1);
    expect(host.querySelector("[data-group-run-control]")).toBeNull();
    await act(async () => host.querySelector("button")!.click());
    expect(onSelect).toHaveBeenCalledOnce();
    await act(async () => root.render(<GroupRunControl group={group} />));
    expect(host.querySelector("button")).toBeNull();
    expect(host.textContent).toBe("statusRunning");
  });

  it("explains why an empty Group cannot be started", async () => {
    await mount(
      <GroupRunControl
        group={{ ...group, running: false }}
        actorCount={0}
        controls={{ pending: null, run: vi.fn(async () => {}) }}
      />,
    );
    await act(async () => host.querySelector("button")!.click());
    expect(document.querySelector('[role="menu"]')?.textContent).toContain("groupRun.noActors");
    expect(document.querySelector<HTMLButtonElement>('[role="menuitem"]')?.disabled).toBe(true);
  });
});
