// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useWorkspaceEditor } from "./useWorkspaceEditor";
import { useWorkspaceNavigation } from "../../stores/workspaceNavigation";

const api = vi.hoisted(() => ({ fetchWorkspaceFile: vi.fn(), saveWorkspaceFile: vi.fn() }));
vi.mock("../../services/api", () => api);

let editor: ReturnType<typeof useWorkspaceEditor>;
let host: HTMLDivElement;
let root: Root;
const noop = () => undefined;
const refresh = vi.fn();

function Harness({
  groupId = "group",
  scopeKey = "scope",
  scopeUrl = "/repo",
}: {
  groupId?: string;
  scopeKey?: string;
  scopeUrl?: string;
}) {
  editor = useWorkspaceEditor(groupId, scopeKey, scopeUrl, refresh, noop, noop);
  return <output>{JSON.stringify(editor.getFileEditStatus("a.txt"))}</output>;
}

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  api.fetchWorkspaceFile.mockImplementation(async (_group, path, scopeKey, scopeUrl) => ({
    ok: true,
    result: {
      scope_key: scopeKey,
      scope_url: scopeUrl,
      path,
      content: `disk ${path}`,
      sha256: "old",
      bytes: 10,
      binary: false,
      truncated: false,
      mime_type: "text/plain",
    },
  }));
  await act(async () => root.render(<Harness />));
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.resetAllMocks();
  refresh.mockClear();
});

it("discards one hidden draft without touching the other file or its navigation protection", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("edited a"));
  await act(async () => editor.openFile("b.txt"));
  await act(async () => editor.setDraft("edited b"));
  expect(editor.getFileEditStatus("a.txt")).toEqual({ dirty: true, saving: false });
  expect(editor.getFileEditStatus("b.txt")).toEqual({ dirty: true, saving: false });

  await act(async () => expect(editor.discardFileDraft("a.txt")).toBe(true));
  expect(editor.getFileEditStatus("a.txt")).toEqual({ dirty: false, saving: false });
  expect(editor.draft).toBe("edited b");
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);
  await act(async () => editor.openFile("a.txt"));
  expect(editor.draft).toBe("disk a.txt");
  await act(async () => editor.openFile("b.txt"));
  expect(editor.draft).toBe("edited b");

  await act(async () => expect(editor.discardFileDraft("b.txt")).toBe(true));
  expect(editor.draft).toBe("disk b.txt");
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
});

it("refuses to discard a file being saved and preserves typing made after the save began", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("submitted"));
  let finish!: (value: unknown) => void;
  api.saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = editor.saveFile(editor.draft);
  });
  await act(async () => editor.setDraft("newer typing"));
  await act(async () => editor.openFile("b.txt"));
  expect(editor.getFileEditStatus("a.txt")).toEqual({ dirty: true, saving: true });
  await act(async () => expect(editor.discardFileDraft("a.txt")).toBe(false));
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved" } });
    await saving;
  });
  expect(editor.getFileEditStatus("a.txt")).toEqual({ dirty: true, saving: false });
  await act(async () => editor.openFile("a.txt"));
  expect(editor.draft).toBe("newer typing");
  await act(async () => expect(editor.discardFileDraft("a.txt")).toBe(true));
  expect(editor.draft).toBe("submitted");
});

it("updates a hidden file's saving status after a failed save, and blocks discard during entry changes", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("edited a"));
  let finish!: (value: unknown) => void;
  api.saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = editor.saveFile(editor.draft);
  });
  await act(async () => editor.openFile("b.txt"));
  await act(async () => editor.setDraft("edited b"));
  expect(host.textContent).toBe('{"dirty":true,"saving":true}');
  await act(async () => {
    finish({ ok: false, error: { code: "FAILED", message: "unavailable" } });
    await saving;
  });
  expect(host.textContent).toBe('{"dirty":true,"saving":false}');
  await act(async () => expect(editor.beginEntryChange()).toBe(true));
  await act(async () => expect(editor.discardFileDraft("a.txt")).toBe(false));
  await act(async () => editor.endEntryChange());
  await act(async () => expect(editor.discardFileDraft("a.txt")).toBe(true));
  expect(editor.getFileEditStatus("b.txt")).toEqual({ dirty: true, saving: false });
});

it("keeps same-named drafts separate across scope changes and exposes an old scope's draft", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("draft in A"));
  await act(async () => root.render(<Harness scopeKey="scope" scopeUrl="/other" />));
  expect(editor.getFileDraft("a.txt", "scope", "/repo")).toBe("draft in A");
  expect(editor.getFileEditStatus("a.txt", "scope", "/repo")).toEqual({
    dirty: true,
    saving: false,
  });
  expect(editor.hasDraftsUnder("")).toBe(false);
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("draft in B"));
  expect(editor.hasDraftsUnder("a.txt")).toBe(true);
  expect(editor.getFileDraft("a.txt", "scope", "/other")).toBe("draft in B");
  await act(async () => expect(editor.discardFileDraft("a.txt", "scope", "/repo")).toBe(true));
  expect(editor.draft).toBe("draft in B");
  expect(editor.getFileDraft("a.txt", "scope", "/repo")).toBeNull();
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);

  // A missing or detached active scope must not make an old draft inaccessible.
  await act(async () => root.render(<Harness scopeKey="" scopeUrl="" />));
  expect(editor.getFileDraft("a.txt", "scope", "/other")).toBe("draft in B");
  await act(async () => root.render(<Harness scopeKey="scope" scopeUrl="/other" />));
  await act(async () => editor.openFile("a.txt"));
  expect(editor.draft).toBe("draft in B");
});

it("settles a pending save in its original scope without changing the new scope's file", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("submitted A"));
  let finish!: (value: unknown) => void;
  api.saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = editor.saveFile(editor.draft);
  });
  await act(async () => editor.setDraft("newer A"));
  await act(async () => root.render(<Harness scopeKey="other" scopeUrl="/other" />));
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("draft B"));
  expect(editor.getFileEditStatus("a.txt", "scope", "/repo")).toEqual({
    dirty: true,
    saving: true,
  });
  await act(async () => expect(editor.discardFileDraft("a.txt", "scope", "/repo")).toBe(false));
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved A" } });
    expect(await saving).toBe(false);
  });
  expect(editor.draft).toBe("draft B");
  expect(editor.file?.sha256).toBe("old");
  expect(refresh).not.toHaveBeenCalled();
  expect(editor.getFileEditStatus("a.txt", "scope", "/repo")).toEqual({
    dirty: true,
    saving: false,
  });
  await act(async () => root.render(<Harness />));
  await act(async () => editor.openFile("a.txt"));
  expect(editor.draft).toBe("newer A");
  expect(editor.file?.sha256).toBe("saved A");
  await act(async () => expect(editor.discardFileDraft("a.txt")).toBe(true));
  expect(editor.draft).toBe("submitted A");
  expect(editor.getFileDraft("a.txt", "other", "/other")).toBe("draft B");
});

it("remaps drafts only within the scope whose entries changed", async () => {
  await act(async () => editor.openFile("dir/a.txt"));
  await act(async () => editor.setDraft("scope A"));
  await act(async () => root.render(<Harness scopeKey="other" scopeUrl="/other" />));
  await act(async () => editor.openFile("dir/a.txt"));
  await act(async () => editor.setDraft("scope B"));
  await act(async () => editor.reconcileEntry("dir", "renamed"));
  expect(editor.getFileDraft("renamed/a.txt", "other", "/other")).toBe("scope B");
  expect(editor.getFileDraft("dir/a.txt", "scope", "/repo")).toBe("scope A");
  expect(editor.getFileDraft("renamed/a.txt", "scope", "/repo")).toBeNull();
});

it("does not carry drafts into a different Group", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("private to group A"));
  await act(async () => root.render(<Harness groupId="other-group" />));
  expect(editor.getFileDraft("a.txt", "scope", "/repo")).toBeNull();
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
});
