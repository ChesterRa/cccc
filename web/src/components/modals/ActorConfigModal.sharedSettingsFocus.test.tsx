// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useModalStore } from "../../stores";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "./ModalFrame";
import type { EditActorConfigProps } from "./ActorConfigModal";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchWebModelBrowserSurfaceSession: vi.fn().mockResolvedValue({ ok: true, result: {} }),
  fetchActorPrivateEnvKeys: vi.fn().mockResolvedValue({ ok: true, result: { keys: [] } }),
  fetchActorProfilePrivateEnvKeys: vi.fn().mockResolvedValue({ ok: true, result: { keys: [] } }),
}));
vi.mock("../CapabilityPicker", () => ({ CapabilityPicker: () => <div /> }));
vi.mock("../RolePresetPicker", () => ({ RolePresetPicker: () => <div /> }));
vi.mock("../ActorAvatarField", () => ({ ActorAvatarField: () => <div /> }));
vi.mock("./RuntimeSelector", () => ({ RuntimeSelector: () => <button>Runtime</button> }));
vi.mock("../webModel/WebModelActorSetup", () => ({
  WebModelActorSetup: ({ onOpenSharedSettings }: { onOpenSharedSettings?: () => void }) => (
    <button onClick={onOpenSharedSettings}>shared-chatgpt</button>
  ),
}));
vi.mock("../webModel/DotsActorPanel", () => ({
  DotsActorPanel: ({ onOpenSharedSettings }: { onOpenSharedSettings?: () => void }) => (
    <button onClick={onOpenSharedSettings}>shared-dots</button>
  ),
}));
import { ActorConfigModal } from "./ActorConfigModal";

function SharedSettingsFixture() {
  const close = () => useModalStore.getState().closeModal("settings");
  const { modalRef } = useModalA11y(true, close);
  return (
    <ModalFrame
      isDark={false}
      onClose={close}
      title="Shared settings"
      titleId="shared-settings-title"
      closeAriaLabel="Close settings"
      panelClassName=""
      modalRef={modalRef}
    >
      <button>Shared configuration</button>
    </ModalFrame>
  );
}

function Fixture({ runtime }: { runtime: "chatgpt_dots" | "web_model" }) {
  const [open, setOpen] = useState(false);
  const settingsOpen = useModalStore((state) => state.modals.settings);
  const props: EditActorConfigProps = {
    mode: "edit",
    isOpen: open,
    isDark: false,
    busy: "",
    groupId: "g_fixture",
    actorId: "actor-fixture",
    isRunning: false,
    savedRuntime: runtime,
    runtime,
    command: "",
    title: "Fixture",
    actorNotes: "",
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
    onCancel: () => setOpen(false),
  };
  return (
    <>
      <button id="actor-controls" onClick={() => setOpen(true)}>
        Actor controls
      </button>
      {settingsOpen ? <SharedSettingsFixture /> : null}
      <ActorConfigModal {...props} />
    </>
  );
}

describe("Actor editor and shared settings focus handoff", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let nextFrame: number;
  let frames: Map<number, FrameRequestCallback>;
  let rejectActorFocus: boolean;
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    useModalStore.getState().closeModal("settings");
    nextFrame = 0;
    frames = new Map();
    rejectActorFocus = false;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
    const focus = HTMLElement.prototype.focus;
    vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (
      this: HTMLElement,
      options?: FocusOptions,
    ) {
      // Chromium rejects focus while the returning ModalFrame's visibility
      // transition is still hidden on its first animation frame.
      if (rejectActorFocus && this.closest('[aria-labelledby="edit-actor-title"]')) return;
      focus.call(this, options);
    });
    const computedStyle = getComputedStyle;
    vi.stubGlobal("getComputedStyle", (element: Element) => {
      const style = computedStyle(element);
      if (element.closest('[aria-labelledby="edit-actor-title"]')) {
        return new Proxy(style, {
          get: (target, key) =>
            key === "visibility"
              ? rejectActorFocus
                ? "hidden"
                : "visible"
              : Reflect.get(target, key),
        });
      }
      return style;
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const frame = async () => {
    const callbacks = [...frames.values()];
    frames.clear();
    await act(async () => callbacks.forEach((callback) => callback(0)));
  };
  const click = async (button: HTMLButtonElement) => {
    button.focus();
    await act(async () => button.click());
  };
  it.each(["chatgpt_dots", "web_model"] as const)(
    "returns to the %s shared-settings trigger after visibility resumes, retaining Escape and Tab boundaries",
    async (runtime) => {
      await act(async () => root.render(<Fixture runtime={runtime} />));
      const controls = host.querySelector<HTMLButtonElement>("#actor-controls")!;
      await click(controls);
      await frame();
      const shared = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
        (button) =>
          button.textContent === (runtime === "chatgpt_dots" ? "shared-dots" : "shared-chatgpt"),
      )!;
      await click(shared);
      await frame();
      expect(
        document.activeElement?.closest('[aria-labelledby="shared-settings-title"]'),
      ).not.toBeNull();
      rejectActorFocus = true;
      await act(async () => {
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
      });
      expect(useModalStore.getState().modals.settings).toBe(false);
      await frame();
      await frame();
      rejectActorFocus = false;
      const editor = host.querySelector<HTMLElement>('[aria-labelledby="edit-actor-title"]')!;
      await act(async () => {
        shared.dispatchEvent(new Event("transitionend", { bubbles: true }));
      });
      await frame();
      expect(document.activeElement).toBe(controls);
      await act(async () => {
        editor.parentElement!.dispatchEvent(new Event("transitionend", { bubbles: true }));
      });
      await frame();
      expect(document.activeElement).toBe(shared);
      const first = editor.querySelector<HTMLButtonElement>('button[aria-label="common:close"]')!;
      const last = [...editor.querySelectorAll<HTMLButtonElement>("button")].find(
        (button) => button.textContent === "common:done",
      )!;
      first.focus();
      await act(async () => {
        editor.parentElement!.dispatchEvent(new Event("transitionend", { bubbles: true }));
      });
      await frame();
      expect(document.activeElement).toBe(first);
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, cancelable: true }),
      );
      expect(document.activeElement).toBe(last);
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", cancelable: true }));
      expect(document.activeElement).toBe(first);
      await act(async () => {
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
      });
      expect(host.querySelector('[aria-labelledby="edit-actor-title"]')).toBeNull();
      expect(document.activeElement).toBe(controls);
    },
  );
});
