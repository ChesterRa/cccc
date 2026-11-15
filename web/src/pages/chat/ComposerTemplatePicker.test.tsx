// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useModalStore } from "../../stores/useModalStore";
import { ComposerTemplatePicker } from "./ComposerTemplatePicker";

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  access: vi.fn(),
  templates: [
    { id: "release", name: "Release notes", text: "\n  release body\n" },
    { id: "meeting", name: "Meeting notes", text: "meeting body" },
  ],
}));
vi.mock("../../features/quickReplies/templateStore", () => ({
  loadQuickReplyTemplates: mocks.load,
  useQuickReplyTemplateStore: <T,>(
    selector: (state: { epoch: number; templates: typeof mocks.templates }) => T,
  ) => selector({ epoch: 0, templates: mocks.templates }),
}));
vi.mock("../../services/api/webAccess", () => ({ fetchWebAccessSession: mocks.access }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const onInsert = vi.fn();
const onRememberSelection = vi.fn();
const openSettingsTarget = vi.fn();
const originalOpenSettings = useModalStore.getState().openSettingsTarget;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  mocks.load.mockResolvedValue({ ok: true });
  mocks.access.mockResolvedValue({
    ok: true,
    result: { web_access_session: { can_access_global_settings: true } },
  });
  useModalStore.setState({ openSettingsTarget });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  useModalStore.setState({ openSettingsTarget: originalOpenSettings });
  host.remove();
});

async function open() {
  await act(async () =>
    root.render(
      <ComposerTemplatePicker
        disabled={false}
        onInsert={onInsert}
        onRememberSelection={onRememberSelection}
      />,
    ),
  );
  const trigger = host.querySelector("button")!;
  await act(async () => {
    trigger.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    trigger.focus();
    trigger.click();
  });
  await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  return trigger;
}

async function search(value: string) {
  const input = document.querySelector<HTMLInputElement>("[cmdk-input]")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return input;
}

it("does not read personal templates or access settings before the list is opened", async () => {
  await act(async () =>
    root.render(
      <ComposerTemplatePicker
        disabled={false}
        onInsert={onInsert}
        onRememberSelection={onRememberSelection}
      />,
    ),
  );
  expect(mocks.load).not.toHaveBeenCalled();
  expect(mocks.access).not.toHaveBeenCalled();
});

it("reloads on each opening and lists the four localized built-ins before personal templates", async () => {
  const trigger = await open();
  expect(onRememberSelection).toHaveBeenCalled();
  expect(mocks.load).toHaveBeenCalledOnce();
  expect([...document.querySelectorAll("[cmdk-item]")].map((item) => item.textContent)).toEqual([
    "quickReply.proceed",
    "quickReply.why",
    "quickReply.rethink",
    "quickReply.choices",
    "Release notes",
    "Meeting notes",
  ]);
  await act(async () => trigger.click());
  await act(async () => trigger.click());
  expect(mocks.load).toHaveBeenCalledTimes(2);
});

it("filters by a name substring and lets arrows and Enter choose the original text", async () => {
  await open();
  const input = await search("NOTES");
  expect([...document.querySelectorAll("[cmdk-item]")].map((item) => item.textContent)).toEqual([
    "Release notes",
    "Meeting notes",
  ]);
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  });
  expect(document.querySelector('[cmdk-item][aria-selected="true"]')?.textContent).toBe(
    "Meeting notes",
  );
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
  });
  expect(document.querySelector('[cmdk-item][aria-selected="true"]')?.textContent).toBe(
    "Release notes",
  );
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  expect(onInsert).toHaveBeenCalledWith("\n  release body\n");
  expect(host.querySelector("button")?.getAttribute("data-state")).toBe("closed");
});

it("does not filter by body or use fuzzy matching, and Escape closes without inserting", async () => {
  const trigger = await open();
  await search("release body");
  expect(document.querySelectorAll("[cmdk-item]")).toHaveLength(0);
  expect(document.body.textContent).toContain("replyTemplates.none");
  const input = await search("rlsn");
  expect(document.querySelectorAll("[cmdk-item]")).toHaveLength(0);
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  expect(onInsert).not.toHaveBeenCalled();
  expect(trigger.getAttribute("data-state")).toBe("closed");
});

it("inserts a built-in's localized body rather than its label", async () => {
  await open();
  await act(async () => document.querySelector<HTMLElement>("[cmdk-item]")!.click());
  expect(onInsert).toHaveBeenCalledWith("quickReply.proceedText");
});

it("offers personal settings to a signed-in non-admin, and hides the link without access", async () => {
  mocks.access.mockResolvedValue({
    ok: true,
    result: {
      web_access_session: {
        can_access_global_settings: false,
        current_browser_signed_in: true,
        login_active: true,
      },
    },
  });
  const trigger = await open();
  const manage = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "replyTemplates.manage",
  )!;
  expect(manage).toBeDefined();
  await act(async () => manage.click());
  expect(openSettingsTarget).toHaveBeenCalledWith({ scope: "global", tab: "quickReplies" });
  mocks.access.mockResolvedValue({ ok: false, error: { code: "permission_denied" } });
  await act(async () => trigger.click());
  expect(document.body.textContent).not.toContain("replyTemplates.manage");
  expect(document.querySelectorAll("[cmdk-item]")).toHaveLength(6);
});
