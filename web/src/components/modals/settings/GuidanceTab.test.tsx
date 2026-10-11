// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

const api = vi.hoisted(() => ({
  fetchGroupPrompts: vi.fn(),
  fetchActors: vi.fn(),
  updateGroupPrompt: vi.fn(),
  resetGroupPrompt: vi.fn(),
}));
vi.mock("../../../services/api", () => api);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  Trans: ({ i18nKey }: { i18nKey: string }) => i18nKey,
}));
import { GuidanceTab } from "./GuidanceTab";

const prompt = (kind: string, content: string, source: "home" | "builtin") => ({
  kind,
  filename: `${kind}.md`,
  path: `/scope/${kind}.md`,
  source,
  content,
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  api.fetchGroupPrompts.mockResolvedValue({
    ok: true,
    result: {
      preamble: prompt("preamble", "Preamble body", "builtin"),
      help: prompt("help", "Shared notes", "home"),
    },
  });
  api.fetchActors.mockResolvedValue({
    ok: true,
    result: { actors: [{ id: "peer-1", title: "Peer One", role: "peer" }] },
  });
  api.updateGroupPrompt.mockImplementation(
    async (_group: string, kind: string, content: string) => ({
      ok: true,
      result: prompt(kind, content, "home"),
    }),
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  vi.unstubAllGlobals();
  await act(async () => root.unmount());
  host.remove();
});
const mount = () => act(async () => root.render(<GuidanceTab isDark={false} groupId="g" />));
const cards = () => [...host.querySelectorAll("[data-settings-card]")];
const button = (label: string, scope: ParentNode = host) =>
  [...scope.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!;
const radio = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find(
    (b) => b.textContent === label,
  )!;
const type = async (textarea: HTMLTextAreaElement, value: string) =>
  act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
      textarea,
      value,
    );
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const groupPrompts = (group: string) => ({
  ok: true,
  result: {
    preamble: prompt("preamble", `${group} preamble`, "home"),
    help: prompt("help", `${group} help`, "home"),
  },
});
const renderGroup = (groupId: string) =>
  act(async () => root.render(<GuidanceTab isDark={false} groupId={groupId} />));

it("gives each prompt card its own actions and resets only a saved override", async () => {
  await mount();
  const [preambleCard, helpCard] = cards();
  for (const card of [preambleCard, helpCard]) {
    const header = card.querySelector("h3")!.parentElement!.parentElement!;
    expect([...header.querySelectorAll("button")].map((b) => b.textContent)).toEqual([
      "guidance.expand",
      "common:reset",
      "guidance.discardChanges",
      "common:save",
    ]);
  }
  // The builtin preamble has no override file to delete; the help override does.
  expect(button("common:reset", preambleCard).disabled).toBe(true);
  expect(button("common:reset", helpCard).disabled).toBe(false);
  expect(preambleCard.textContent).toContain("/scope/preamble.md");

  vi.stubGlobal(
    "confirm",
    vi.fn(() => false),
  );
  await act(async () => button("common:reset", helpCard).click());
  expect(window.confirm).toHaveBeenCalledWith("automation.resetGuidanceConfirm");
  expect(api.resetGroupPrompt).not.toHaveBeenCalled();
});

it("edits help by scope with one-tap choices and saves only the changed block", async () => {
  await mount();
  const helpCard = cards()[1];
  expect(radio("guidance.structuredView").getAttribute("aria-checked")).toBe("true");
  expect(radio("guidance.commonNotesTitle").getAttribute("aria-checked")).toBe("true");
  expect(radio("Peer One")).toBeDefined();

  await act(async () => radio("guidance.foremanNotesTitle").click());
  expect(radio("guidance.foremanNotesTitle").getAttribute("aria-checked")).toBe("true");
  const editor = helpCard.querySelectorAll("textarea")[0];
  expect(editor.value).toBe("");
  await type(editor, "Lead the work");
  await act(async () => button("common:save", helpCard).click());
  expect(api.updateGroupPrompt).toHaveBeenCalledWith(
    "g",
    "help",
    expect.stringContaining("Lead the work"),
    { editorMode: "structured", changedBlocks: ["role:foreman"] },
  );

  // Raw mode shows the whole file in one editor.
  await act(async () => radio("guidance.rawView").click());
  const raw = helpCard.querySelectorAll("textarea");
  expect(raw).toHaveLength(1);
  expect(raw[0].value).toContain("Shared notes");
  expect(raw[0].value).toContain("Lead the work");
});

it("gives the expanded editor its own ids so its labels reach the dialog's controls", async () => {
  await mount();
  for (const index of [0, 1]) {
    await act(async () => button("guidance.expand", cards()[index]).click());
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    const ids = [...document.querySelectorAll("[id]")].map((element) => element.id);
    expect(ids.length).toBe(new Set(ids).size);
    const references = [
      ...[...dialog.querySelectorAll("label[for]")].map((label) => label.getAttribute("for")!),
      ...[...dialog.querySelectorAll("[aria-labelledby]")].map(
        (element) => element.getAttribute("aria-labelledby")!,
      ),
    ];
    expect(references.length).toBeGreaterThan(0);
    for (const id of references) {
      expect(dialog.contains(document.getElementById(id))).toBe(true);
    }
    await act(async () => button("common:close", dialog).click());
  }
});

it("does not let a late Group A read replace Group B guidance or become its saved content", async () => {
  const oldRead = deferred<ReturnType<typeof groupPrompts>>();
  api.fetchGroupPrompts.mockImplementation((group: string) =>
    group === "A" ? oldRead.promise : Promise.resolve(groupPrompts(group)),
  );
  await renderGroup("A");
  await renderGroup("B");
  expect(host.querySelectorAll("textarea")[0].value).toBe("B preamble");
  await act(async () => oldRead.resolve(groupPrompts("A")));
  expect(host.querySelectorAll("textarea")[0].value).toBe("B preamble");
  await act(async () => button("common:save", cards()[0]).click());
  expect(api.updateGroupPrompt).toHaveBeenCalledWith("B", "preamble", "B preamble");
});

it("clears previous Group guidance while the next Group read is pending", async () => {
  const nextRead = deferred<ReturnType<typeof groupPrompts>>();
  api.fetchGroupPrompts.mockImplementation((group: string) =>
    group === "B" ? nextRead.promise : Promise.resolve(groupPrompts(group)),
  );
  await renderGroup("A");
  await type(host.querySelectorAll("textarea")[0], "Unsaved A draft");
  await renderGroup("B");
  expect(host.querySelectorAll("textarea")[0].value).toBe("");
  expect(button("common:save", cards()[0]).disabled).toBe(true);
  await act(async () => nextRead.resolve(groupPrompts("B")));
  expect(host.querySelectorAll("textarea")[0].value).toBe("B preamble");
});

it.each(["read", "save"] as const)(
  "ignores a departed Group's %s failure and busy cleanup while the current Group loads",
  async (operation) => {
    const oldOperation = deferred<
      ReturnType<typeof groupPrompts> | { ok: false; error: { message: string } }
    >();
    const nextRead = deferred<ReturnType<typeof groupPrompts>>();
    api.fetchGroupPrompts.mockImplementation((group: string) => {
      if (group === "B") return nextRead.promise;
      return operation === "read" ? oldOperation.promise : Promise.resolve(groupPrompts("A"));
    });
    api.updateGroupPrompt.mockImplementation(() => oldOperation.promise);
    await renderGroup("A");
    if (operation === "save") await act(async () => button("common:save", cards()[0]).click());
    await renderGroup("B");
    await act(async () => oldOperation.resolve({ ok: false, error: { message: "Old A failure" } }));
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(button("common:save", cards()[0]).disabled).toBe(true);
    await act(async () => nextRead.resolve(groupPrompts("B")));
    expect(host.querySelectorAll("textarea")[0].value).toBe("B preamble");
    expect(button("common:save", cards()[0]).disabled).toBe(false);
  },
);
