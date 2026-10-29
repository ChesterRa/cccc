// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { EditActorConfigProps } from "../../components/modals/ActorConfigModal";
import { NativeActorEditor } from "./NativeActorEditor";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let host: HTMLDivElement;
let root: Root;
const props = (): EditActorConfigProps => ({
  mode: "edit",
  isOpen: true,
  isDark: false,
  busy: "",
  groupId: "g_fixture",
  actorId: "codex-1",
  isRunning: true,
  savedRuntime: "codex",
  runtime: "codex",
  command: "",
  title: "Native draft",
  actorNotes: "Original notes",
  capabilityAutoloadText: "",
  runtimes: [],
  actorProfiles: [],
  actorProfilesBusy: false,
  onChangeRuntime: vi.fn(),
  onChangeCommand: vi.fn(),
  onChangeTitle: vi.fn(),
  onChangeActorNotes: vi.fn(),
  onChangeCapabilityAutoloadText: vi.fn(),
  onSaveAsProfile: vi.fn(),
  onSave: vi.fn(),
  onSaveAndRestart: vi.fn(),
  onCancel: vi.fn(),
});
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const saveButton = () =>
  Array.from(host.querySelectorAll("button")).find(
    (button) => button.textContent === "common:save",
  )!;
it("waits for Group notes and preserves the metadata draft when save fails", async () => {
  const config = props();
  config.onSave = vi.fn().mockRejectedValue(new Error("Synthetic save failure"));
  await act(async () => root.render(<NativeActorEditor {...config} actorNotesBusy />));
  expect(saveButton().disabled).toBe(true);
  expect(host.textContent).toContain("loadingActorNotes");
  expect(host.querySelector("fieldset")?.getAttribute("aria-busy")).toBe("true");
  expect(host.querySelector("textarea")?.disabled).toBe(true);
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="common:close"]')?.disabled).toBe(
    false,
  );
  await act(async () => root.render(<NativeActorEditor {...config} actorNotesBusy={false} />));
  expect(host.textContent).not.toContain("loadingActorNotes");
  await act(async () => saveButton().click());
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("Synthetic save failure");
  expect(host.querySelector("input")?.value).toBe("Native draft");
  expect(host.querySelector("textarea")?.value).toBe("Original notes");
  expect(saveButton().disabled).toBe(false);
  expect(host.querySelector("fieldset")?.getAttribute("aria-busy")).toBe("false");
  expect(config.onSave).toHaveBeenCalledExactlyOnceWith({
    mode: "custom",
    setVars: {},
    unsetKeys: [],
    clear: false,
    capabilityAutoload: [],
    runtimeMode: "default",
  });
  expect(config.onSaveAndRestart).not.toHaveBeenCalled();
  expect(config.onSaveAsProfile).not.toHaveBeenCalled();
});
it("keeps a pending save from closing or accepting another submission", async () => {
  const config = props();
  let finish!: () => void;
  config.onSave = vi.fn().mockReturnValue(
    new Promise<void>((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => root.render(<NativeActorEditor {...config} />));
  await act(async () => saveButton().click());
  const pending = Array.from(host.querySelectorAll("button")).find(
    (button) => button.textContent === "common:saving",
  )!;
  expect(pending.disabled).toBe(true);
  expect(pending.getAttribute("aria-busy")).toBe("true");
  expect(host.querySelector("fieldset")?.disabled).toBe(true);
  const close = host.querySelector<HTMLButtonElement>('button[aria-label="common:close"]')!;
  expect(close.disabled).toBe(true);
  expect(close.className).toContain("disabled:opacity-50");
  await act(async () => pending.click());
  expect(config.onSave).toHaveBeenCalledOnce();
  await act(async () => close.click());
  await act(async () =>
    host
      .querySelector(".glass-overlay")
      ?.dispatchEvent(new Event("pointerdown", { bubbles: true })),
  );
  await act(async () =>
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
  );
  expect(config.onCancel).not.toHaveBeenCalled();
  await act(async () => finish());
  expect(saveButton().disabled).toBe(false);
  expect(close.disabled).toBe(false);
  await act(async () => close.click());
  expect(config.onCancel).toHaveBeenCalledOnce();
});
