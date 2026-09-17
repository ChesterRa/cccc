// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { GroupWorkArea, type RuntimeActorView } from "./GroupWorkArea";
import { terminalPageLayout } from "./groupWorkLayout";
import { useUIStore } from "../../stores/useUIStore";

vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;
let host: HTMLDivElement;
let root: Root;
let controlsHost: HTMLDivElement;
let panelsHost: HTMLDivElement;
let width = 1200;
let resize: () => void;
const actors = Array.from({ length: 8 }, (_, index) => ({ id: `actor-${index + 1}` }));

function FixtureActor({
  id,
  groupId,
  isVisible,
  onExpand,
}: RuntimeActorView & { id: string; groupId: string }) {
  return (
    <div>
      <span id={`runtime-inspector-${groupId}-${id}`}>{id}</span>
      <button data-expand={id} onClick={onExpand}>
        expand
      </button>
      <div className="xterm">
        <textarea data-terminal={id} data-visible={String(isVisible)} />
      </div>
    </div>
  );
}

function Fixture({ groupId = "g1", count = 8, loading = false, covered = false }) {
  const [active, setActive] = useState("chat");
  return (
    <GroupWorkArea
      groupId={groupId}
      actors={actors.slice(0, count)}
      availableGroupIds={["g1", "g2"]}
      activeActorId={active === "chat" ? undefined : active}
      isDark={false}
      isVisible
      covered={covered}
      loading={loading}
      workControlsHost={controlsHost}
      sidePanelControlsHost={panelsHost}
      isSmallScreen={false}
      sidePanelControls={<button>presentation</button>}
      onInspectActor={setActive}
      renderActor={(id, view) => <FixtureActor id={id} groupId={groupId} {...view} />}
    >
      <div data-message>message history</div>
    </GroupWorkArea>
  );
}

async function render(
  props: { groupId?: string; count?: number; loading?: boolean; covered?: boolean } = {},
) {
  await act(async () => {
    root.render(<Fixture {...props} />);
  });
}
async function click(selector: string) {
  await act(async () => {
    (host.querySelector<HTMLButtonElement>(selector) ||
      controlsHost.querySelector<HTMLButtonElement>(selector))!.click();
  });
}

beforeEach(() => {
  localStorage.clear();
  useUIStore.setState({ chatSessions: {}, activeTab: "chat" });
  width = 1200;
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => width);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(cb: () => void) {
        resize = cb;
      }
      observe() {}
      disconnect() {}
    },
  );
  host = document.createElement("div");
  document.body.appendChild(host);
  controlsHost = document.createElement("div");
  document.body.appendChild(controlsHost);
  panelsHost = document.createElement("div");
  document.body.appendChild(panelsHost);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  controlsHost.remove();
  panelsHost.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Group work area", () => {
  it("paginates at four and keeps each group's view and page through switching", async () => {
    await render();
    expect(host.querySelectorAll("[data-terminal]")).toHaveLength(0);
    await click('[aria-label="workView.label"] button:last-child');
    expect(host.querySelectorAll('[data-terminal][data-visible="true"]')).toHaveLength(4);
    const first = host.querySelector<HTMLTextAreaElement>('[data-terminal="actor-1"]')!;
    first.value = "retained input";
    await click('[aria-label="workView.next"]');
    expect(host.querySelector('[data-terminal="actor-5"]')).not.toBeNull();
    expect(host.querySelector('[data-terminal="actor-1"]')).toBe(first);
    expect(first.closest("[inert]")).not.toBeNull();
    await render({ groupId: "g2" });
    expect(host.querySelectorAll('[data-terminal][data-visible="true"]')).toHaveLength(0);
    await click('[aria-label="workView.label"] button:last-child');
    const second = host.querySelector('[data-runtime-group-id="g2"] [data-terminal="actor-1"]');
    expect(second).not.toBe(first);
    expect(second).not.toBeNull();
    await render({ groupId: "g1" });
    expect(host.querySelector('[data-terminal="actor-5"]')).not.toBeNull();
    expect(useUIStore.getState().chatSessions.g1.terminalPage).toBe(1);
    expect(host.querySelector("[data-group-message-view]")?.getAttribute("inert")).not.toBeNull();
    await click('[aria-label="workView.previous"]');
    expect(host.querySelector('[data-runtime-group-id="g1"] [data-terminal="actor-1"]')).toBe(
      first,
    );
    expect(first.value).toBe("retained input");
  });

  it("maximizes and restores the same terminal, retaining input and terminal keys", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await render({ count: 4 });
    const terminal = host.querySelector<HTMLTextAreaElement>('[data-terminal="actor-2"]')!;
    terminal.value = "unfinished input";
    await click('[data-expand="actor-2"]');
    expect(host.querySelector('[data-terminal="actor-2"]')).toBe(terminal);
    expect(host.querySelector('[role="dialog"][aria-modal="true"]')).not.toBeNull();
    terminal.focus();
    for (const key of ["Escape", "Tab"]) {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      terminal.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }
    await click('[aria-label="workView.restore"]');
    expect(host.querySelector('[data-terminal="actor-2"]')).toBe(terminal);
    expect(terminal.value).toBe("unfinished input");
    expect(host.querySelector('[aria-modal="true"]')).toBeNull();
  });

  it("suspends hidden tiles and retains the current page DOM across view changes", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await render({ count: 2 });
    await act(async () => new Promise((resolve) => setTimeout(resolve, 10)));
    const terminal = host.querySelector('[data-terminal="actor-1"]');
    await click('[aria-label="workView.label"] button:first-child');
    expect(host.querySelector('[data-terminal="actor-1"]')).toBe(terminal);
    expect(host.querySelectorAll('[data-visible="true"]')).toHaveLength(0);
    await click('[aria-label="workView.label"] button:last-child');
    expect(host.querySelector('[data-terminal="actor-1"]')).toBe(terminal);
  });

  it("clamps stale pages without erasing preferences during hydration and adapts to narrow space", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    useUIStore.getState().setGroupTerminalPage("g1", 1);
    await render({ count: 0, loading: true });
    expect(useUIStore.getState().chatSessions.g1.terminalPage).toBe(1);
    await render();
    expect(host.querySelector('[data-terminal="actor-5"]')).not.toBeNull();
    width = 390;
    await act(async () => resize());
    expect(host.querySelectorAll('[data-terminal][data-visible="true"]')).toHaveLength(1);
    expect(terminalPageLayout(2, 9, 1200)).toMatchObject({ page: 0, pageCount: 1 });
    expect(panelsHost.textContent).toContain("presentation");
    expect(controlsHost.textContent).not.toContain("presentation");
    expect(host.querySelector('[data-terminal="actor-5"]')).not.toBeNull();
    expect(useUIStore.getState().chatSessions.g1.terminalPage).toBe(4);
  });
});

it("makes the covered work area inert while keeping portalled header controls usable", async () => {
  await render({ covered: true });
  const area = host.querySelector("[data-group-work-area]")!;
  expect(area.hasAttribute("inert")).toBe(true);
  expect(area.getAttribute("aria-hidden")).toBe("true");
  expect(controlsHost.querySelector("button")!.closest("[inert]")).toBeNull();
  expect(panelsHost.querySelector("button")!.closest("[inert]")).toBeNull();
  await render({ covered: false });
  expect(area.hasAttribute("inert")).toBe(false);
});
