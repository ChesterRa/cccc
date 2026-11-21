// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useModalStore } from "../../stores/useModalStore";
import { ComposerTemplatePicker } from "./ComposerTemplatePicker";
import type { QuickReplyEntry } from "../../features/quickReplies/effective";

const RELEASE = { id: "release", name: "Release notes", text: "\n  release body\n" };
const MEETING = { id: "meeting", name: "Meeting notes", text: "meeting body" };
const DEFAULTS: QuickReplyEntry[] = [
  { builtin: "proceed" },
  { builtin: "why" },
  { builtin: "rethink" },
  { builtin: "choices" },
];
const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  access: vi.fn(),
  replies: [] as QuickReplyEntry[],
  loaded: true,
  loadError: null as string | null,
}));
vi.mock("../../features/quickReplies/templateStore", () => ({
  loadQuickReplyTemplates: mocks.load,
  useQuickReplyTemplateStore: <T,>(
    selector: (state: {
      epoch: number;
      replies: QuickReplyEntry[];
      loaded: boolean;
      loadError: string | null;
    }) => T,
  ) =>
    selector({
      epoch: 0,
      replies: mocks.replies,
      loaded: mocks.loaded,
      loadError: mocks.loadError,
    }),
}));
vi.mock("../../services/api/webAccess", () => ({ fetchWebAccessSession: mocks.access }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const onInsert = vi.fn();
const onRememberSelection = vi.fn();
const getAnchorRect = vi.fn(() => new DOMRect(300, 600, 0, 0));
const openSettingsTarget = vi.fn();
const originalOpenSettings = useModalStore.getState().openSettingsTarget;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  mocks.replies = [...DEFAULTS, RELEASE, MEETING];
  mocks.loaded = true;
  mocks.loadError = null;
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

function picker(wide = false) {
  return (
    <ComposerTemplatePicker
      disabled={false}
      getAnchorRect={getAnchorRect}
      wide={wide}
      onInsert={onInsert}
      onRememberSelection={onRememberSelection}
    />
  );
}

function names() {
  return [...document.querySelectorAll("[cmdk-item]")].map(
    (item) => item.querySelector("[data-template-name]")?.textContent,
  );
}

function selectedName() {
  return document
    .querySelector('[cmdk-item][aria-selected="true"]')
    ?.querySelector("[data-template-name]")?.textContent;
}

async function open(wide = false) {
  await act(async () => root.render(picker(wide)));
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
  await act(async () => root.render(picker()));
  expect(mocks.load).not.toHaveBeenCalled();
  expect(mocks.access).not.toHaveBeenCalled();
});

it("reloads on each opening and lists the replies in their saved order, in one group", async () => {
  const trigger = await open();
  expect(onRememberSelection).toHaveBeenCalled();
  expect(mocks.load).toHaveBeenCalledOnce();
  expect(names()).toEqual([
    "quickReply.proceed",
    "quickReply.why",
    "quickReply.rethink",
    "quickReply.choices",
    "Release notes",
    "Meeting notes",
  ]);
  expect(document.querySelector("[cmdk-group-heading]")).toBeNull();
  await act(async () => trigger.click());
  mocks.replies = [MEETING, { builtin: "why" }, RELEASE];
  await act(async () => trigger.click());
  expect(mocks.load).toHaveBeenCalledTimes(2);
  expect(names()).toEqual(["Meeting notes", "quickReply.why", "Release notes"]);
});

it("filters by a name substring and lets arrows and Enter choose the original text", async () => {
  await open();
  const input = await search("NOTES");
  expect(names()).toEqual(["Release notes", "Meeting notes"]);
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  });
  expect(selectedName()).toBe("Meeting notes");
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
  });
  expect(selectedName()).toBe("Release notes");
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

it("uses changed default fields, leaves out a deleted default and inserts saved text verbatim", async () => {
  mocks.replies = [
    { builtin: "proceed", name: "Check first", text: "\n  saved body\n" },
    { builtin: "rethink" },
    { builtin: "choices" },
    RELEASE,
    MEETING,
  ];
  await open();
  expect(names()).toEqual([
    "Check first",
    "quickReply.rethink",
    "quickReply.choices",
    "Release notes",
    "Meeting notes",
  ]);
  await act(async () => document.querySelector<HTMLElement>("[cmdk-item]")!.click());
  expect(onInsert).toHaveBeenCalledWith("\n  saved body\n");
});

it("offers no defaults before settings are known and retries on the next opening", async () => {
  mocks.loaded = false;
  const trigger = await open();
  expect(document.querySelectorAll("[cmdk-item]")).toHaveLength(0);
  expect(document.body.textContent).toContain("common:loading");
  expect(onInsert).not.toHaveBeenCalled();
  mocks.loadError = "read failed";
  await act(async () => trigger.click());
  await act(async () => trigger.click());
  expect(document.body.textContent).toContain("replyTemplates.unavailable");
  expect(document.querySelectorAll("[cmdk-item]")).toHaveLength(0);
  mocks.loaded = true;
  mocks.loadError = null;
  mocks.replies = [{ builtin: "proceed", name: "Recovered" }, RELEASE];
  await act(async () => trigger.click());
  await act(async () => trigger.click());
  expect(mocks.load).toHaveBeenCalledTimes(3);
  expect(names()[0]).toBe("Recovered");
});

it("shows each template's first line of text under its name", async () => {
  await open();
  const rows = [...document.querySelectorAll("[cmdk-item]")].map((item) => [
    item.querySelector("[data-template-name]")?.textContent,
    item.querySelector("[data-template-preview]")?.textContent ?? null,
  ]);
  expect(rows[0]).toEqual(["quickReply.proceed", "quickReply.proceedText"]);
  expect(rows.slice(-2)).toEqual([
    ["Release notes", "release body"],
    ["Meeting notes", "meeting body"],
  ]);
});

it("opens above the input near the insertion point, and spans the input on a phone", async () => {
  await open();
  expect(getAnchorRect).toHaveBeenCalled();
  const list = document.querySelector<HTMLElement>("[data-lcd-menu]")!;
  expect(list.getAttribute("data-side")).toBe("top");
  expect(list.className).toContain("w-80");
  expect(list.style.width).toBe("");
  await act(async () => root.render(picker(true)));
  const wide = document.querySelector<HTMLElement>("[data-lcd-menu]")!;
  expect(wide.className).not.toContain("w-80");
  expect(wide.style.width).toBe("var(--radix-popover-trigger-width)");
});
