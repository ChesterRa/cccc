// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { resetQuickReplyTemplatesForTest } from "../../../features/quickReplies/templateStore";
import { QuickRepliesTab } from "./QuickRepliesTab";

const api = vi.hoisted(() => ({ fetch: vi.fn(), save: vi.fn() }));
vi.mock("../../../services/api/quickReplyTemplates", () => ({
  fetchQuickReplyTemplates: api.fetch,
  saveQuickReplyTemplates: api.save,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

type Template = { id: string; name: string; text: string };
const list = (templates: Template[], revision: number) => ({
  ok: true,
  result: { templates, revision, updated_at: "2026-11-15T00:00:00Z", limit: 20 },
});

let root: ReturnType<typeof createRoot>, host: HTMLDivElement;

async function open(templates: Template[], revision = 3) {
  api.fetch.mockResolvedValue(list(templates, revision));
  await act(async () => root.render(<QuickRepliesTab />));
}

const rows = () => Array.from(host.querySelectorAll<HTMLElement>("[data-quick-reply-row]"));
const nameOf = (row: HTMLElement) => row.querySelector<HTMLInputElement>("input")!;
const textOf = (row: HTMLElement) => row.querySelector<HTMLTextAreaElement>("textarea")!;
const shownNames = () => rows().map((row) => nameOf(row).value);
const button = (row: HTMLElement, label: string) =>
  row.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
const saveButton = () => host.querySelector<HTMLButtonElement>("[data-settings-save-bar] button")!;
const buttonNamed = (text: string) =>
  Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
    (item) => item.textContent === text,
  )!;

async function type(field: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype =
    field instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function click(target: HTMLElement) {
  await act(async () => target.click());
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  api.fetch.mockReset();
  api.save.mockReset();
  resetQuickReplyTemplatesForTest();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("saves added, edited, moved and deleted templates in order over the revision it read", async () => {
  await open([
    { id: "a", name: "Alpha", text: "Alpha text" },
    { id: "b", name: "Beta", text: "Beta text" },
  ]);
  expect(shownNames()).toEqual(["Alpha", "Beta"]);
  expect(saveButton().disabled).toBe(true);

  await click(buttonNamed("quickReplies.add"));
  const added = rows()[2];
  expect(document.activeElement).toBe(nameOf(added));
  await type(nameOf(added), "Gamma");
  // The text is sent as written, line breaks and indentation included.
  const written = "\n  Gamma:\n    - first\n";
  await type(textOf(added), written);
  await click(button(rows()[2], "quickReplies.moveUp"));
  expect(shownNames()).toEqual(["Alpha", "Gamma", "Beta"]);
  await click(button(rows()[0], "common:delete"));
  await type(textOf(rows()[1]), "Beta edited");
  expect(saveButton().disabled).toBe(false);

  api.save.mockResolvedValue(
    list(
      [
        { id: "g", name: "Gamma", text: written },
        { id: "b", name: "Beta", text: "Beta edited" },
      ],
      4,
    ),
  );
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(3, [
    { name: "Gamma", text: written },
    { id: "b", name: "Beta", text: "Beta edited" },
  ]);
  expect(shownNames()).toEqual(["Gamma", "Beta"]);
  expect(saveButton().disabled).toBe(true);
  expect(host.textContent).toContain("saveFeedback.saved");
});

it("reports a save made elsewhere, keeps the edits, and replaces them only on reload", async () => {
  await open([{ id: "a", name: "Alpha", text: "Alpha text" }]);
  await type(nameOf(rows()[0]), "Alpha mine");
  api.save.mockResolvedValue({
    ok: false,
    error: { code: "quick_reply_templates_conflict", message: "changed", details: { revision: 5 } },
  });
  await click(saveButton());
  expect(host.querySelector('[data-settings-notice="warning"]')?.textContent).toContain(
    "quickReplies.conflict",
  );
  expect(shownNames()).toEqual(["Alpha mine"]);
  expect(saveButton().disabled).toBe(true);

  api.fetch.mockResolvedValue(list([{ id: "a", name: "Alpha theirs", text: "Alpha text" }], 5));
  await click(buttonNamed("quickReplies.reload"));
  expect(host.querySelector('[data-settings-notice="warning"]')).toBeNull();
  expect(shownNames()).toEqual(["Alpha theirs"]);
});

it("offers nothing to add until the list is read, then saves over the revision read", async () => {
  api.fetch.mockResolvedValueOnce({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "offline" },
  });
  await act(async () => root.render(<QuickRepliesTab />));
  expect(host.querySelector('[data-settings-notice="error"]')?.textContent).toContain(
    "quickReplies.loadFailed",
  );
  expect(buttonNamed("quickReplies.add").disabled).toBe(true);

  api.fetch.mockResolvedValueOnce(list([{ id: "a", name: "Alpha", text: "Alpha text" }], 7));
  await click(buttonNamed("common:retry"));
  expect(host.querySelector('[data-settings-notice="error"]')).toBeNull();
  expect(shownNames()).toEqual(["Alpha"]);
  await click(buttonNamed("quickReplies.add"));
  await type(nameOf(rows()[1]), "Beta");
  await type(textOf(rows()[1]), "Beta text");
  api.save.mockResolvedValue(
    list(
      [
        { id: "a", name: "Alpha", text: "Alpha text" },
        { id: "b", name: "Beta", text: "Beta text" },
      ],
      8,
    ),
  );
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(7, [
    { id: "a", name: "Alpha", text: "Alpha text" },
    { name: "Beta", text: "Beta text" },
  ]);
});

it("points at each problem instead of saving", async () => {
  await open([{ id: "a", name: "Alpha", text: "Alpha text" }]);
  await type(nameOf(rows()[0]), "   ");
  await click(buttonNamed("quickReplies.add"));
  await type(nameOf(rows()[1]), "Blank");
  await type(textOf(rows()[1]), " \n\t ");
  await click(saveButton());
  expect(api.save).not.toHaveBeenCalled();
  expect(rows()[0].textContent).toContain("quickReplies.problem.name");
  expect(nameOf(rows()[0]).getAttribute("aria-invalid")).toBe("true");
  expect(rows()[1].textContent).toContain("quickReplies.problem.text");
  expect(textOf(rows()[1]).getAttribute("aria-invalid")).toBe("true");
});
