// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { TFunction } from "i18next";
import { VoiceDocumentLibrary } from "./VoiceDocumentLibrary";
import { voiceDocumentLibrary } from "../../../services/api/voiceDocumentLibrary";
vi.mock("../../../services/api/voiceDocumentLibrary", () => ({ voiceDocumentLibrary: vi.fn() }));
const t = ((_key: string, options: Record<string, unknown>) =>
  String(options.defaultValue || _key).replace("{{count}}", String(options.count))) as TFunction;
const active = {
  document_id: "a",
  document_path: "voice/a.md",
  title: "Active notes",
  status: "active",
  folder_id: "f",
};
const archived = {
  document_id: "b",
  document_path: "voice/b.md",
  title: "Archived notes",
  content: "# Kept archive",
  status: "archived",
};
const data = { folders: [{ folder_id: "f", name: "Meetings" }], documents: [active, archived] };
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
let props: ComponentProps<typeof VoiceDocumentLibrary>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(voiceDocumentLibrary).mockReset().mockResolvedValue({ ok: true, result: data });
  props = {
    groupId: "g",
    documents: [active],
    actionBusy: "",
    activeDocumentPath: "",
    captureTargetDocumentPath: "",
    creatingDocument: false,
    isDark: false,
    newDocumentTitleDraft: "",
    t,
    documentKey: (d) => d.document_id,
    documentPath: (d) => d.document_path || "",
    onCancelCreateDocument: vi.fn(),
    onCreateDocument: vi.fn(),
    onNewDocumentTitleChange: vi.fn(),
    onSelectDocument: vi.fn(),
    onSetCaptureTargetDocument: vi.fn(),
    onStartCreateDocument: vi.fn(),
    onRestored: vi.fn(),
    onDeleteDocument: vi.fn(),
  };
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});
function button(text: string) {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    b.textContent?.includes(text),
  )!;
}

it("navigates persisted folders without showing physical paths", async () => {
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  expect(host.textContent).not.toContain("Active notes");
  await act(async () => button("Meetings").click());
  expect(host.textContent).toContain("Active notes");
  expect(host.textContent).not.toContain("voice/a.md");
  await act(async () => host.querySelector<HTMLElement>('[role="button"]')!.click());
  expect(props.onSelectDocument).toHaveBeenCalledWith(active);
});

it("offers archive restore separately from delete and passes the exact target", async () => {
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  await act(async () => button("Archived documents").click());
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain("Archived notes");
  await act(async () => button("Delete").click());
  expect(props.onDeleteDocument).toHaveBeenCalledWith(archived);
  expect(props.onRestored).not.toHaveBeenCalled();
  await act(async () => button("Restore").click());
  expect(voiceDocumentLibrary).toHaveBeenCalledWith("g", {
    action: "restore",
    document_path: "voice/b.md",
  });
  expect(props.onRestored).toHaveBeenCalledWith({ ...archived, status: "active" });
});

it("shows failed folder creation inside its dialog and keeps it open", async () => {
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  await act(async () => button("New folder").click());
  const input = document.querySelector<HTMLInputElement>('input[aria-label="New folder"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      input,
      "Duplicate",
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  vi.mocked(voiceDocumentLibrary).mockResolvedValue({
    ok: false,
    error: { code: "invalid_args", message: "Folder name already exists" },
  });
  await act(async () =>
    document
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(document.querySelector('[role="dialog"] [role="alert"]')?.textContent).toBe(
    "Folder name already exists",
  );
  expect(button("Save").disabled).toBe(false);
});

it("renames a document from its row menu and reports the rename", async () => {
  props.onRenamed = vi.fn();
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  await act(async () => button("Meetings").click());
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[role="button"] button[aria-haspopup="menu"]')!.click(),
  );
  const rename = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(
    (item) => item.textContent === "Rename",
  )!;
  expect(rename).toBeTruthy();
  await act(async () => rename.click());
  const input = document.querySelector<HTMLInputElement>('input[aria-label="Rename document"]')!;
  expect(input.value).toBe("Active notes");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      input,
      "Renamed notes",
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Save").click());
  expect(voiceDocumentLibrary).toHaveBeenCalledWith("g", {
    action: "rename",
    document_path: "voice/a.md",
    name: "Renamed notes",
  });
  expect(props.onRenamed).toHaveBeenCalledTimes(1);
  expect(document.querySelector('input[aria-label="Rename document"]')).toBeNull();
});
