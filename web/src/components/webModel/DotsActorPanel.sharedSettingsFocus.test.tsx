// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useModalStore } from "../../stores/useModalStore";
import { useModalA11y } from "../../hooks/useModalA11y";
import { RuntimeInspectorModal } from "../modals/RuntimeInspectorModal";
import { ModalFrame } from "../modals/ModalFrame";

const mocks = vi.hoisted(() => ({ update: vi.fn(), t: (key: string) => key }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: mocks.t, i18n: { language: "en" } }),
}));
vi.mock("../../services/api", () => ({
  fetchWebModelConnectors: async () => ({
    ok: true,
    result: {
      connectors: [{ connector_id: "shared", provider: "chatgpt_web" }],
      connector_base_url: "https://cccc.example.test",
    },
  }),
  fetchDotsActorState: async () => ({
    ok: true,
    result: {
      group_id: "g_fixture",
      actor_id: "dot",
      enabled: true,
      connected: false,
      subscribed: false,
      phase: "unconfigured",
      pending_count: 0,
    },
  }),
  updateDotsActorConnection: (...args: unknown[]) => mocks.update(...args),
}));
import { DotsActorPanel } from "./DotsActorPanel";

function SharedSettings() {
  const close = () => useModalStore.getState().closeModal("settings");
  const { modalRef } = useModalA11y(true, close);
  return (
    <ModalFrame
      isDark={false}
      onClose={close}
      title="Shared ChatGPT setup"
      titleId="shared-settings-title"
      closeAriaLabel="Close settings"
      panelClassName=""
      modalRef={modalRef}
    >
      <button>Fixture shared connector</button>
    </ModalFrame>
  );
}
function Fixture() {
  const [open, setOpen] = useState(false);
  const settingsOpen = useModalStore((state) => state.modals.settings);
  return (
    <>
      <button id="inspect-dot" onClick={() => setOpen(true)}>
        Inspect Dot
      </button>
      <RuntimeInspectorModal
        isOpen={open}
        isDark={false}
        onClose={() => setOpen(false)}
        titleId="dot-title"
        closeAriaLabel="Restore workspace"
      >
        <span id="dot-title">Dot</span>
        <DotsActorPanel
          groupId="g_fixture"
          actorId="dot"
          isVisible={open && !settingsOpen}
          onOpenSharedSettings={() =>
            useModalStore
              .getState()
              .openSettingsTarget({
                scope: "global",
                tab: "webModels",
                webModelProvider: "chatgpt_web",
              })
          }
        />
      </RuntimeInspectorModal>
      {settingsOpen ? <SharedSettings /> : null}
    </>
  );
}
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  vi.clearAllMocks();
  useModalStore.getState().closeModal("settings");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
it("keeps Settings keyboard focus and Escape above the runtime inspector, then returns to the shared-setup trigger", async () => {
  await act(async () => root.render(<Fixture />));
  const inspect = host.querySelector<HTMLButtonElement>("#inspect-dot")!;
  inspect.focus();
  await act(async () => inspect.click());
  await act(async () => vi.advanceTimersByTimeAsync(20));
  const shared = [...host.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "dotsActor.sharedSettings",
  )!;
  expect(shared).toBeDefined();
  shared.focus();
  await act(async () => shared.click());
  await act(async () => vi.advanceTimersByTimeAsync(20));
  const dialog = host.querySelector<HTMLElement>('[aria-labelledby="shared-settings-title"]')!;
  expect(dialog.contains(document.activeElement)).toBe(true);
  const first = dialog.querySelector<HTMLButtonElement>('button[aria-label="Close settings"]')!;
  const last = [...dialog.querySelectorAll<HTMLButtonElement>("button")].at(-1)!;
  last.focus();
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", cancelable: true }));
  expect(document.activeElement).toBe(first);
  document.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, cancelable: true }),
  );
  expect(document.activeElement).toBe(last);
  await act(async () =>
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true })),
  );
  await act(async () => vi.advanceTimersByTimeAsync(20));
  expect(useModalStore.getState().modals.settings).toBe(false);
  expect(host.querySelector('[aria-labelledby="dot-title"]')).not.toBeNull();
  expect(document.activeElement).toBe(shared);
  expect(mocks.update).not.toHaveBeenCalled();
  await act(async () =>
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true })),
  );
  expect(document.activeElement).toBe(inspect);
});
