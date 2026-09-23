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
function mouse(target: Element | Document, type: string, y: number) {
  return act(async () => {
    target.dispatchEvent(
      new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        button: 0,
        buttons: type === "mouseup" ? 0 : 1,
        clientX: 10,
        clientY: y,
      }),
    );
  });
}
function documentRow(title: string) {
  return [...document.querySelectorAll<HTMLElement>('div[role="button"]')].find((row) =>
    row.textContent?.includes(title),
  )!;
}
function button(text: string) {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    b.textContent?.includes(text),
  )!;
}

it("expands persisted folders inline without showing physical paths", async () => {
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  const folder = button("Meetings");
  expect(folder.getAttribute("aria-expanded")).toBe("false");
  expect(host.textContent).not.toContain("Active notes");
  await act(async () => folder.click());
  expect(folder.getAttribute("aria-expanded")).toBe("true");
  expect(host.textContent).toContain("Active notes");
  expect(host.textContent).not.toContain("voice/a.md");
  await act(async () => documentRow("Active notes").click());
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
  await act(async () =>
    host.querySelector<HTMLButtonElement>('button[aria-label="New folder"]')!.click(),
  );
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
    documentRow("Active notes")
      .querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!
      .click(),
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

it("moves a document by dragging it onto a folder and expands that folder", async () => {
  const unfiled = { ...active, document_id: "c", document_path: "voice/c.md", title: "Loose" };
  delete (unfiled as { folder_id?: string }).folder_id;
  props.documents = [active, unfiled];
  vi.mocked(voiceDocumentLibrary).mockResolvedValue({
    ok: true,
    result: { ...data, documents: [active, unfiled, archived] },
  });
  // happy-dom has no layout: place the folder row above the unfiled document.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const top = this.matches("[data-voice-folder-group]") ? 0 : 100;
    return DOMRect.fromRect({ x: 0, y: top, width: 200, height: 30 });
  });
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  const row = documentRow("Loose");
  await mouse(row, "mousedown", 110);
  await mouse(document, "mousemove", 60);
  await mouse(document, "mousemove", 10);
  await mouse(document, "mouseup", 10);
  expect(voiceDocumentLibrary).toHaveBeenCalledWith("g", {
    action: "move",
    document_path: "voice/c.md",
    folder_id: "f",
  });
  expect(button("Meetings").getAttribute("aria-expanded")).toBe("true");
});

it("reorders folders by dragging a folder below the others", async () => {
  const folders = ["A", "B", "C"].map((name) => ({ folder_id: name.toLowerCase(), name }));
  props.documents = [];
  vi.mocked(voiceDocumentLibrary).mockResolvedValue({
    ok: true,
    result: { folders, documents: [] },
  });
  // Stack folder groups 40px apart in their render order.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const groups = [...document.querySelectorAll("[data-voice-folder-group]")];
    const index = groups.indexOf(this);
    return DOMRect.fromRect({ x: 0, y: index < 0 ? 200 : index * 40, width: 200, height: 30 });
  });
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  const folderA = button("A");
  await mouse(folderA, "mousedown", 15);
  await mouse(document, "mousemove", 60);
  await mouse(document, "mousemove", 105);
  await mouse(document, "mouseup", 105);
  expect(voiceDocumentLibrary).toHaveBeenCalledWith("g", {
    action: "reorder_root",
    root_order: ["folder:b", "folder:c", "folder:a"],
  });
  // A drag that ends as a drag must not also toggle the folder open.
  expect(folderA.getAttribute("aria-expanded")).toBe("false");
});

it("drags a folder below an unfiled document and renders the saved mixed order", async () => {
  const loose = { ...active, document_id: "c", document_path: "voice/c.md", title: "Loose" };
  delete (loose as { folder_id?: string }).folder_id;
  props.documents = [loose];
  vi.mocked(voiceDocumentLibrary).mockResolvedValue({
    ok: true,
    result: { folders: [{ folder_id: "f", name: "Meetings" }], documents: [loose] },
  });
  // Stack root items (folder group or document slot) 40px apart in render order.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const items = [
      ...document.querySelectorAll("[data-voice-folder-group],[data-voice-root-slot]"),
    ];
    const index = items.indexOf(this);
    return DOMRect.fromRect({ x: 0, y: index < 0 ? 200 : index * 40, width: 200, height: 30 });
  });
  await act(async () => root.render(<VoiceDocumentLibrary {...props} />));
  const order = () =>
    [...host.querySelectorAll("[data-voice-folder-group],[data-voice-root-slot]")].map(
      (item) => item.textContent,
    );
  // Unsaved items keep the old layout: folders first.
  expect(order()[0]).toContain("Meetings");
  vi.mocked(voiceDocumentLibrary).mockResolvedValue({
    ok: true,
    result: {
      folders: [{ folder_id: "f", name: "Meetings" }],
      documents: [loose],
      root_order: ["document:voice/c.md", "folder:f"],
    },
  });
  await mouse(button("Meetings"), "mousedown", 15);
  await mouse(document, "mousemove", 35);
  await mouse(document, "mousemove", 55);
  await mouse(document, "mouseup", 55);
  expect(voiceDocumentLibrary).toHaveBeenCalledWith("g", {
    action: "reorder_root",
    root_order: ["document:voice/c.md", "folder:f"],
  });
  expect(order()[0]).toContain("Loose");
  expect(order()[1]).toContain("Meetings");
});
