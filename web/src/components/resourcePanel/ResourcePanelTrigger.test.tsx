// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";
import { ResourcePanelTrigger } from "./ResourcePanelTrigger";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { open?: number }) =>
      options?.open === undefined ? key : `${key}:${options.open}`,
  }),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  useResourcePanelStore.setState({ groups: {} });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("stays with no tab open and toggles the empty panel itself", () => {
  const onToggle = vi.fn();
  const render = (shown: boolean) =>
    act(() => root.render(<ResourcePanelTrigger groupId="g1" shown={shown} onToggle={onToggle} />));
  render(false);
  const trigger = () => host.querySelector<HTMLButtonElement>("[data-resource-panel-toggle]")!;
  expect(trigger().textContent).toBe("0");
  expect(trigger().getAttribute("aria-label")).toBe("resourcePanel.showEmpty");
  expect(trigger().getAttribute("aria-haspopup")).toBeNull();
  act(() => trigger().click());
  expect(onToggle).toHaveBeenCalledTimes(1);
  render(true);
  expect(trigger().getAttribute("aria-label")).toBe("resourcePanel.hideEmpty");
  expect(trigger().getAttribute("aria-expanded")).toBe("true");
});
