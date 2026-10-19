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
let height = 860;
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

function Fixture({
  groupId = "g1",
  count = 8,
  loading = false,
  covered = false,
  errorActorId = "",
  webActorId = "",
}) {
  const [active, setActive] = useState("chat");
  return (
    <GroupWorkArea
      groupId={groupId}
      actors={actors
        .slice(0, count)
        .map((actor) =>
          actor.id === errorActorId
            ? { ...actor, effective_working_state: "error" }
            : actor.id === webActorId
              ? { ...actor, runtime: "web_model", runner: "headless" }
              : actor,
        )}
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
  props: {
    groupId?: string;
    count?: number;
    loading?: boolean;
    covered?: boolean;
    errorActorId?: string;
    webActorId?: string;
  } = {},
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
  height = 860;
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => width);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(() => height);
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
  it("waits for work-area measurement before mounting tiled terminals", async () => {
    width = 0;
    height = 0;
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await render();
    expect(host.querySelectorAll("[data-terminal]")).toHaveLength(0);
    width = 1200;
    height = 540;
    await act(async () => resize());
    expect(host.querySelectorAll('[data-terminal][data-visible="true"]')).toHaveLength(2);
  });

  it("keeps an off-page failed owner reachable and clears attention once it is in view", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await render({ errorActorId: "actor-8" });
    const attention = controlsHost.querySelector<HTMLButtonElement>(
      '[aria-label="workView.otherAttention"]',
    )!;
    expect(attention).not.toBeNull();
    await act(async () => attention.click());
    expect(useUIStore.getState().chatSessions.g1.terminalPage).toBe(1);
    expect(host.querySelector('[data-terminal="actor-8"][data-visible="true"]')).not.toBeNull();
    expect(controlsHost.querySelector('[aria-label="workView.otherAttention"]')).toBeNull();
    await click('[aria-label="workView.previous"]');
    expect(controlsHost.querySelector('[aria-label="workView.otherAttention"]')).not.toBeNull();
    await render();
    expect(controlsHost.querySelector('[aria-label="workView.otherAttention"]')).toBeNull();
  });
  it("paginates by the panes that fit and keeps each group's view and page through switching", async () => {
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

  it.each([
    [1200, 860, 4],
    [1920, 540, 2],
    [1200, 540, 2],
    [900, 1000, 1],
    [390, 844, 1],
  ])("keeps panes readable in a %i by %i work area", (availableWidth, availableHeight, count) => {
    expect(terminalPageLayout(8, 0, availableWidth, availableHeight).pageSize).toBe(count);
  });

  it("uses two rows for three or four panes, with at most two columns", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await render({ count: 3 });
    const grid = host.querySelector<HTMLElement>('[data-group-terminal-view="visible"]')!;
    expect(grid.style.gridTemplateColumns).toBe("repeat(2,minmax(0,1fr))");
    expect(grid.style.gridTemplateRows).toBe("repeat(2,minmax(0,1fr))");
    expect(host.querySelectorAll('[data-terminal][data-visible="true"]')).toHaveLength(3);
    height = 540;
    await act(async () => resize());
    expect(grid.style.gridTemplateRows).toBe("minmax(0,1fr)");
    expect(host.querySelectorAll('[data-terminal][data-visible="true"]')).toHaveLength(2);
  });

  it("retains the focused terminal and its input when height reduces the page capacity", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    useUIStore.getState().setGroupTerminalPage("g1", 1);
    await render();
    const terminal = host.querySelector<HTMLTextAreaElement>('[data-terminal="actor-7"]')!;
    terminal.value = "retained input";
    terminal.focus();
    height = 540;
    await act(async () => resize());
    expect(useUIStore.getState().chatSessions.g1.terminalPage).toBe(3);
    expect(host.querySelector('[data-terminal="actor-7"][data-visible="true"]')).toBe(terminal);
    expect(document.activeElement).toBe(terminal);
    expect(terminal.value).toBe("retained input");
    height = 860;
    await act(async () => resize());
    expect(useUIStore.getState().chatSessions.g1.terminalPage).toBe(1);
    expect(host.querySelector('[data-terminal="actor-7"][data-visible="true"]')).toBe(terminal);
    expect(terminal.value).toBe("retained input");
  });

  it("keeps Actor order when a non-retained Web Model returns to the grid", async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    await render({ count: 4, webActorId: "actor-3" });
    height = 540;
    await act(async () => resize());
    expect(host.querySelector('[data-terminal="actor-3"]')).toBeNull();
    height = 860;
    await act(async () => resize());
    expect(
      [...host.querySelectorAll('[data-terminal][data-visible="true"]')].map((node) =>
        node.getAttribute("data-terminal"),
      ),
    ).toEqual(["actor-1", "actor-2", "actor-3", "actor-4"]);
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
    expect(terminalPageLayout(2, 9, 1200, 860)).toMatchObject({ page: 0, pageCount: 1 });
    expect(panelsHost.textContent).toContain("presentation");
    expect(controlsHost.textContent).not.toContain("presentation");
    expect(host.querySelector('[data-terminal="actor-5"]')).not.toBeNull();
    // Page 1 of four panes began at actor-5; one pane keeps that terminal in view.
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
