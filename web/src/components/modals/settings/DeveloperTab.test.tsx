// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { DeveloperTab } from "./DeveloperTab";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

type Props = ComponentProps<typeof DeveloperTab>;
const baseProps = (): Props => ({
  isDark: false,
  groupId: "g1",
  runtimeVersion: "1.0.0",
  daemonVersion: "1.0.0",
  runtimeInfoErr: "",
  developerMode: false,
  setDeveloperMode: vi.fn(),
  logLevel: "INFO",
  setLogLevel: vi.fn(),
  terminalBacklogMiB: 10,
  setTerminalBacklogMiB: vi.fn(),
  terminalScrollbackLines: 8000,
  setTerminalScrollbackLines: vi.fn(),
  peerRuntimeVisibility: "visible",
  setPeerRuntimeVisibility: vi.fn(),
  assistantRuntimeVisibility: "hidden",
  setAssistantRuntimeVisibility: vi.fn(),
  obsBusy: false,
  onSaveObservability: vi.fn(),
  debugSnapshot: "",
  debugSnapshotErr: "",
  debugSnapshotBusy: false,
  onLoadDebugSnapshot: vi.fn(),
  onClearDebugSnapshot: vi.fn(),
  logComponent: "daemon",
  setLogComponent: vi.fn(),
  logLines: 200,
  setLogLines: vi.fn(),
  logText: "",
  logErr: "",
  logBusy: false,
  onLoadLogTail: vi.fn(),
  onClearLogs: vi.fn(),
  registryBusy: false,
  registryErr: "",
  registryResult: null,
  onPreviewRegistry: vi.fn(),
  onReconcileRegistry: vi.fn(),
});

let host: HTMLDivElement;
let root: Root;
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
const render = (props: Props) => act(async () => root.render(<DeveloperTab {...props} />));
const button = (label: string, scope: ParentNode = host) =>
  [...scope.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!;
const card = (title: string) =>
  [...host.querySelectorAll("[data-settings-card]")].find(
    (node) => node.querySelector("h3")?.textContent === title,
  )!;

it("switches developer mode and gates the diagnostics on it", async () => {
  const props = baseProps();
  await render(props);
  const toggle = host.querySelector('[role="switch"]')!;
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  await act(async () => (toggle as HTMLElement).click());
  expect(props.setDeveloperMode).toHaveBeenCalledWith(true);
  expect(button("developer.refresh", card("developer.debugSnapshot")).disabled).toBe(true);
  expect(button("developer.refresh", card("developer.logTail")).disabled).toBe(true);

  await render({ ...props, developerMode: true });
  expect(button("developer.refresh", card("developer.debugSnapshot")).disabled).toBe(false);
  await render({ ...props, developerMode: true, groupId: undefined });
  // The snapshot is group scoped; without a group it cannot load.
  expect(button("developer.refresh", card("developer.debugSnapshot")).disabled).toBe(true);
  expect(host.textContent).toContain("developer.openFromGroup");
});

it("offers two- and three-way choices as one-tap segments", async () => {
  const props = baseProps();
  await render(props);
  const groups = [...host.querySelectorAll('[role="radiogroup"]')];
  const labels = groups.map((group) =>
    [...group.querySelectorAll('[role="radio"]')].map((radio) => radio.textContent),
  );
  expect(labels).toEqual([
    ["INFO", "DEBUG"],
    ["developer.visible", "developer.hidden"],
    ["developer.hidden", "developer.visible"],
    ["daemon", "web", "im"],
  ]);
  await act(async () => button("DEBUG").click());
  expect(props.setLogLevel).toHaveBeenCalledWith("DEBUG");
  await act(async () => button("im").click());
  expect(props.setLogComponent).toHaveBeenCalledWith("im");
});

it("falls back to the default buffer size when the field is cleared", async () => {
  const props = baseProps();
  await render(props);
  const backlog = host.querySelector<HTMLInputElement>('input[type="number"][max="50"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(backlog, "");
    backlog.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(props.setTerminalBacklogMiB).toHaveBeenCalledWith(10);
});

it("saves the developer settings from the page save bar", async () => {
  const props = baseProps();
  await render(props);
  const save = host.querySelector<HTMLButtonElement>("[data-settings-save-bar] button")!;
  expect(save.textContent).toBe("developer.saveDeveloperSettings");
  await act(async () => save.click());
  expect(props.onSaveObservability).toHaveBeenCalledOnce();
  await render({ ...props, obsBusy: true });
  expect(host.querySelector<HTMLButtonElement>("[data-settings-save-bar] button")!.disabled).toBe(
    true,
  );
});

it("keeps destructive maintenance in danger cards with their guards", async () => {
  const props = baseProps();
  await render(props);
  const danger = [...host.querySelectorAll('[data-settings-card="danger"]')];
  expect(danger.map((node) => node.querySelector("h3")?.textContent)).toEqual([
    "developer.clearTruncate",
    "developer.registryTitle",
  ]);
  expect(button("developer.clearTruncate", danger[0]).disabled).toBe(true);
  // Cleaning only applies to a scan that found missing groups.
  expect(button("developer.cleanMissing", danger[1]).disabled).toBe(true);

  await render({
    ...props,
    developerMode: true,
    registryResult: {
      dry_run: true,
      scanned_groups: 3,
      missing_group_ids: ["g-missing"],
      corrupt_group_ids: [],
      removed_group_ids: [],
      removed_default_scope_keys: [],
    },
  });
  await act(async () => button("developer.clearTruncate", danger[0]).click());
  expect(props.onClearLogs).toHaveBeenCalledOnce();
  await act(async () => button("developer.cleanMissing").click());
  expect(props.onReconcileRegistry).toHaveBeenCalledOnce();
  expect(host.textContent).toContain("g-missing");
});
