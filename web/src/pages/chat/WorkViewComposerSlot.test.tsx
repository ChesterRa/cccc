// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { WorkViewComposerSlot } from "./WorkViewComposerSlot";
import { useUIStore } from "../../stores/useUIStore";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  await act(async () => {
    useUIStore.getState().setGroupWorkView("g1", "messages");
    useUIStore.getState().setGroupWorkView("g2", "messages");
  });
});

it("hides the composer in the terminals view and keeps its draft for the return", async () => {
  await act(async () =>
    root.render(
      <WorkViewComposerSlot groupId="g1">
        <textarea data-composer />
      </WorkViewComposerSlot>,
    ),
  );
  const slot = () => host.querySelector<HTMLElement>("[data-work-view-composer]")!;
  const box = host.querySelector<HTMLTextAreaElement>("[data-composer]")!;
  expect(slot().hidden).toBe(false);
  box.value = "half-written";

  await act(async () => useUIStore.getState().setGroupWorkView("g1", "terminals"));
  expect(slot().hidden).toBe(true);
  expect(slot().hasAttribute("inert")).toBe(true);

  // Another Group's view leaves this Group's composer alone.
  await act(async () => useUIStore.getState().setGroupWorkView("g2", "messages"));
  expect(slot().hidden).toBe(true);

  await act(async () => useUIStore.getState().setGroupWorkView("g1", "messages"));
  expect(slot().hidden).toBe(false);
  expect(slot().hasAttribute("inert")).toBe(false);
  // Same element, never remounted: the draft is still there.
  expect(host.querySelector<HTMLTextAreaElement>("[data-composer]")).toBe(box);
  expect(box.value).toBe("half-written");
});

it("reveals a hidden composer when an external action targets it without affecting other Groups", async () => {
  await act(async () => {
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    useUIStore.getState().setGroupWorkView("g2", "terminals");
    root.render(
      <WorkViewComposerSlot groupId="g1">
        <textarea data-composer />
      </WorkViewComposerSlot>,
    );
  });
  const slot = () => host.querySelector<HTMLElement>("[data-work-view-composer]")!;
  const box = host.querySelector<HTMLTextAreaElement>("[data-composer]")!;
  expect(slot().hidden).toBe(true);
  expect(slot().hasAttribute("inert")).toBe(true);

  await act(async () => useUIStore.getState().setGroupWorkView("g1", "messages"));
  expect(slot().hidden).toBe(false);
  expect(slot().hasAttribute("inert")).toBe(false);
  box.focus();
  expect(document.activeElement).toBe(box);
  expect(useUIStore.getState().chatSessions.g2?.workView).toBe("terminals");
});
