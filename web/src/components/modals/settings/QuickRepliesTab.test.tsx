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

type Entry =
  | { builtin: string; name?: string; text?: string }
  | { id: string; name: string; text: string };
const DEFAULTS: Entry[] = [
  { builtin: "proceed" },
  { builtin: "why" },
  { builtin: "rethink" },
  { builtin: "choices" },
];
const ALPHA = { id: "a", name: "Alpha", text: "Alpha text" };
const BETA = { id: "b", name: "Beta", text: "Beta text" };
const list = (replies: Entry[], revision: number) => ({
  ok: true,
  result: { replies, revision, updated_at: "2026-11-15T00:00:00Z", limit: 24 },
});

let root: ReturnType<typeof createRoot>, host: HTMLDivElement;

async function open(replies: Entry[], revision = 3) {
  api.fetch.mockResolvedValue(list(replies, revision));
  await act(async () => root.render(<QuickRepliesTab />));
}

const rows = () => Array.from(host.querySelectorAll<HTMLElement>("[data-quick-reply-row]"));
const builtin = (kind: string) =>
  host.querySelector<HTMLElement>(`[data-quick-reply-builtin="${kind}"]`)!;
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
// The mocked translation returns keys, so a default reply's name reads as its key.
const bringBack = () =>
  Array.from(host.querySelectorAll<HTMLButtonElement>("[data-quick-reply-bring-back] button")).map(
    (item) => item.textContent,
  );

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

it("shows one list in saved order and saves added, edited, moved and deleted replies", async () => {
  await open([...DEFAULTS, ALPHA, BETA]);
  expect(shownNames()).toEqual([
    "quickReply.proceed",
    "quickReply.why",
    "quickReply.rethink",
    "quickReply.choices",
    "Alpha",
    "Beta",
  ]);
  expect(saveButton().disabled).toBe(true);
  // All four default replies are in the list, so there is nothing to bring back.
  expect(bringBack()).toEqual([]);
  // Only a default reply has a restore button, and it comes first, so the move and delete
  // buttons keep one column when default replies and your own are mixed.
  const labels = (row: HTMLElement) =>
    Array.from(row.querySelectorAll("button")).map((item) => item.getAttribute("aria-label"));
  expect(labels(builtin("proceed"))).toEqual([
    "quickReplies.restore",
    "quickReplies.moveUp",
    "quickReplies.moveDown",
    "common:delete",
  ]);
  expect(labels(rows()[4])).toEqual([
    "quickReplies.moveUp",
    "quickReplies.moveDown",
    "common:delete",
  ]);

  await click(buttonNamed("quickReplies.add"));
  const added = rows()[6];
  expect(document.activeElement).toBe(nameOf(added));
  await type(nameOf(added), "Gamma");
  // The text is sent as written, line breaks and indentation included.
  const written = "\n  Gamma:\n    - first\n";
  await type(textOf(added), written);
  await click(button(rows()[6], "quickReplies.moveUp"));
  await click(button(builtin("proceed"), "common:delete"));
  await type(textOf(rows()[5]), "Beta edited");
  expect(shownNames()).toEqual([
    "quickReply.why",
    "quickReply.rethink",
    "quickReply.choices",
    "Alpha",
    "Gamma",
    "Beta",
  ]);
  expect(saveButton().disabled).toBe(false);

  api.save.mockResolvedValue(
    list(
      [
        ...DEFAULTS.slice(1),
        ALPHA,
        { id: "g", name: "Gamma", text: written },
        { id: "b", name: "Beta", text: "Beta edited" },
      ],
      4,
    ),
  );
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(3, [
    { builtin: "why" },
    { builtin: "rethink" },
    { builtin: "choices" },
    ALPHA,
    { name: "Gamma", text: written },
    { id: "b", name: "Beta", text: "Beta edited" },
  ]);
  expect(shownNames()).toEqual([
    "quickReply.why",
    "quickReply.rethink",
    "quickReply.choices",
    "Alpha",
    "Gamma",
    "Beta",
  ]);
  expect(saveButton().disabled).toBe(true);
  expect(host.textContent).toContain("saveFeedback.saved");
});

it("moves a default reply below the person's own", async () => {
  await open([{ builtin: "proceed" }, ALPHA]);
  expect(button(builtin("proceed"), "quickReplies.moveUp").disabled).toBe(true);
  await click(button(builtin("proceed"), "quickReplies.moveDown"));
  expect(shownNames()).toEqual(["Alpha", "quickReply.proceed"]);
  api.save.mockResolvedValue(list([ALPHA, { builtin: "proceed" }], 4));
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(3, [ALPHA, { builtin: "proceed" }]);
});

it("deletes a default reply and brings it back at the end as its default", async () => {
  await open([{ builtin: "proceed", name: "Go" }, { builtin: "why" }, ALPHA]);
  expect(bringBack()).toEqual(["quickReply.rethink", "quickReply.choices"]);
  await click(button(builtin("proceed"), "common:delete"));
  expect(bringBack()).toEqual(["quickReply.proceed", "quickReply.rethink", "quickReply.choices"]);
  await click(buttonNamed("quickReply.proceed"));
  expect(shownNames()).toEqual(["quickReply.why", "Alpha", "quickReply.proceed"]);
  expect(button(builtin("proceed"), "quickReplies.restore").disabled).toBe(true);
  api.save.mockResolvedValue(list([{ builtin: "why" }, ALPHA, { builtin: "proceed" }], 4));
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(3, [{ builtin: "why" }, ALPHA, { builtin: "proceed" }]);
});

it("counts the default replies toward the limit", async () => {
  const mine = Array.from({ length: 20 }, (_, index) => ({
    id: `m${index}`,
    name: `Mine ${index}`,
    text: `Text ${index}`,
  }));
  await open([...DEFAULTS, ...mine]);
  expect(buttonNamed("quickReplies.add").disabled).toBe(true);
  await click(button(builtin("why"), "common:delete"));
  expect(buttonNamed("quickReplies.add").disabled).toBe(false);
  expect(buttonNamed("quickReply.why").disabled).toBe(false);
  await click(buttonNamed("quickReplies.add"));
  expect(buttonNamed("quickReplies.add").disabled).toBe(true);
  // Full again: the deleted default reply cannot come back until there is room.
  expect(buttonNamed("quickReply.why").disabled).toBe(true);
});

it("keeps an empty list empty and offers all four default replies to bring back", async () => {
  await open([]);
  expect(rows()).toHaveLength(0);
  expect(host.textContent).toContain("quickReplies.empty");
  expect(bringBack()).toEqual([
    "quickReply.proceed",
    "quickReply.why",
    "quickReply.rethink",
    "quickReply.choices",
  ]);
  expect(saveButton().disabled).toBe(true);
});

it("reports a save made elsewhere, keeps the edits, and replaces them only on reload", async () => {
  await open([ALPHA]);
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

  api.fetch.mockResolvedValue(list([{ ...ALPHA, name: "Alpha theirs" }], 5));
  await click(buttonNamed("quickReplies.reload"));
  expect(host.querySelector('[data-settings-notice="warning"]')).toBeNull();
  expect(shownNames()).toEqual(["Alpha theirs"]);
});

it("offers nothing to edit or add until the list is read, then saves over the revision read", async () => {
  api.fetch.mockResolvedValueOnce({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "offline" },
  });
  await act(async () => root.render(<QuickRepliesTab />));
  expect(host.querySelector('[data-settings-notice="error"]')?.textContent).toContain(
    "quickReplies.loadFailed",
  );
  expect(buttonNamed("quickReplies.add").disabled).toBe(true);
  // Which default replies the list holds is unknown until it is read.
  expect(rows()).toHaveLength(0);
  expect(bringBack()).toEqual([]);

  api.fetch.mockResolvedValueOnce(list([ALPHA], 7));
  await click(buttonNamed("common:retry"));
  expect(host.querySelector('[data-settings-notice="error"]')).toBeNull();
  expect(shownNames()).toEqual(["Alpha"]);
  await click(buttonNamed("quickReplies.add"));
  await type(nameOf(rows()[1]), "Beta");
  await type(textOf(rows()[1]), "Beta text");
  api.save.mockResolvedValue(list([ALPHA, BETA], 8));
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(7, [ALPHA, { name: "Beta", text: "Beta text" }]);
});

it("points at each problem instead of saving", async () => {
  await open([{ builtin: "rethink" }, ALPHA]);
  await type(nameOf(builtin("rethink")), " ");
  await type(nameOf(rows()[1]), "   ");
  await click(buttonNamed("quickReplies.add"));
  await type(nameOf(rows()[2]), "Blank");
  await type(textOf(rows()[2]), " \n\t ");
  await click(saveButton());
  expect(api.save).not.toHaveBeenCalled();
  expect(builtin("rethink").textContent).toContain("quickReplies.problem.name");
  expect(nameOf(builtin("rethink")).getAttribute("aria-invalid")).toBe("true");
  expect(rows()[1].textContent).toContain("quickReplies.problem.name");
  expect(rows()[2].textContent).toContain("quickReplies.problem.text");
  expect(textOf(rows()[2]).getAttribute("aria-invalid")).toBe("true");
});

it("shows default replies in the interface language and saves only the fields that differ", async () => {
  await open([...DEFAULTS, ALPHA]);
  expect(nameOf(builtin("proceed")).value).toBe("quickReply.proceed");
  expect(textOf(builtin("proceed")).value).toBe("quickReply.proceedText");
  expect(button(builtin("proceed"), "quickReplies.restore").disabled).toBe(true);
  // Only a default reply can be restored.
  expect(button(rows()[4], "quickReplies.restore")).toBeNull();

  // A name typed back to its default is no change.
  await type(nameOf(builtin("proceed")), "Go");
  await type(nameOf(builtin("proceed")), "quickReply.proceed");
  expect(saveButton().disabled).toBe(true);

  await type(nameOf(builtin("proceed")), "  Go  ");
  await type(textOf(builtin("choices")), "Give me options.\n");
  // The daemon trims the name, as it does for a reply of the person's own.
  api.save.mockResolvedValue(
    list(
      [
        { builtin: "proceed", name: "Go" },
        { builtin: "why" },
        { builtin: "rethink" },
        { builtin: "choices", text: "Give me options.\n" },
        ALPHA,
      ],
      4,
    ),
  );
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(3, [
    { builtin: "proceed", name: "  Go  " },
    { builtin: "why" },
    { builtin: "rethink" },
    { builtin: "choices", text: "Give me options.\n" },
    ALPHA,
  ]);
  expect(nameOf(builtin("proceed")).value).toBe("Go");
  expect(saveButton().disabled).toBe(true);
});

it("restores one default reply and leaves the rest of the list", async () => {
  await open([{ builtin: "proceed", name: "Go", text: "Go on." }, { builtin: "why" }, ALPHA], 5);
  expect(nameOf(builtin("proceed")).value).toBe("Go");
  await click(button(builtin("proceed"), "quickReplies.restore"));
  expect(nameOf(builtin("proceed")).value).toBe("quickReply.proceed");
  expect(textOf(builtin("proceed")).value).toBe("quickReply.proceedText");
  expect(button(builtin("proceed"), "quickReplies.restore").disabled).toBe(true);

  api.save.mockResolvedValue(list([{ builtin: "proceed" }, { builtin: "why" }, ALPHA], 6));
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(5, [{ builtin: "proceed" }, { builtin: "why" }, ALPHA]);
  expect(shownNames()).toEqual(["quickReply.proceed", "quickReply.why", "Alpha"]);
});

it("keeps a stored change that reads like the default of the language shown now", async () => {
  // Saved in another interface language; in this one it happens to match the default text.
  const stored = { builtin: "proceed", text: "quickReply.proceedText" };
  await open([stored], 2);
  expect(saveButton().disabled).toBe(true);
  expect(host.textContent).not.toContain("quickReplies.unsaved");
  expect(button(builtin("proceed"), "quickReplies.restore").disabled).toBe(false);

  await click(buttonNamed("quickReplies.add"));
  await type(nameOf(rows()[1]), "Mine");
  await type(textOf(rows()[1]), "My text");
  api.save.mockResolvedValue(list([stored, { id: "m", name: "Mine", text: "My text" }], 3));
  await click(saveButton());
  expect(api.save).toHaveBeenCalledWith(2, [stored, { name: "Mine", text: "My text" }]);
});
