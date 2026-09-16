// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { workspacePreviewKind } from "./workspacePreview";
import { useWorkspaceFiles } from "./useWorkspaceFiles";
import { useWorkspaceNavigation } from "../../stores/workspaceNavigation";
const api = vi.hoisted(() => ({
  resolveWorkspacePath: vi.fn(),
  fetchWorkspaceListing: vi.fn(),
  fetchWorkspaceFile: vi.fn(),
  saveWorkspaceFile: vi.fn(),
  changeWorkspaceEntry: vi.fn(),
  uploadWorkspaceFile: vi.fn(),
  fetchWorkspaceChanges: vi.fn(),
  fetchWorkspaceDiff: vi.fn(),
}));
vi.mock("../../services/api", () => api);
vi.mock("../../services/api/workspace", () => api);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let files: ReturnType<typeof useWorkspaceFiles>;
function Harness({ scope = "scope" }: { scope?: string }) {
  files = useWorkspaceFiles("group", true, scope, "/repo");
  return null;
}
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const diskFile = (path: string) => ({
  scope_key: "scope",
  scope_url: "/repo",
  path,
  content: "original",
  sha256: "sha",
  bytes: 8,
  binary: false,
  truncated: false,
  mime_type: "text/plain",
});
beforeEach(async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.resetAllMocks();
  api.fetchWorkspaceListing.mockResolvedValue({
    ok: true,
    result: { items: [], path: "", root_path: "/repo" },
  });
  api.resolveWorkspacePath.mockImplementation(async (_g, path) => ({
    ok: true,
    result: { path, is_dir: true },
  }));
  api.fetchWorkspaceFile.mockImplementation(async (_group, path) => ({
    ok: true,
    result: diskFile(path),
  }));
  api.changeWorkspaceEntry.mockImplementation(async (_g, _k, _u, op) => ({
    ok: true,
    result: { path: op.path, destination: op.destination },
  }));
  api.uploadWorkspaceFile.mockResolvedValue({ ok: true, result: { path: "new.txt", bytes: 1 } });
  api.fetchWorkspaceChanges.mockResolvedValue({
    ok: true,
    result: { repository: true, branch: "main", entries: [], limited: false },
  });
  api.fetchWorkspaceDiff.mockResolvedValue({
    ok: true,
    result: { patch: "patch", limited: false },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Harness />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
describe("workspace mutation ownership", () => {
  it("moves all descendant drafts and clears only explicitly deleted drafts", async () => {
    await act(async () => {
      await files.openFile("src/a.txt");
    });
    await act(async () => files.setDraft("draft A"));
    await act(async () => {
      await files.openFile("other.txt");
    });
    await act(async () => files.setDraft("draft other"));
    await act(async () => {
      await files.mutations.change({ operation: "move", path: "src", destination: "lib" });
    });
    await act(async () => {
      await files.openFile("lib/a.txt");
    });
    expect(files.draft).toBe("draft A");
    expect(files.file?.path).toBe("lib/a.txt");
    await act(async () => {
      await files.mutations.change({ operation: "delete", path: "lib" });
    });
    expect(files.file).toBeNull();
    expect(files.hasDraftsUnder("lib")).toBe(false);
    await act(async () => {
      await files.openFile("other.txt");
    });
    expect(files.draft).toBe("draft other");
    expect(useWorkspaceNavigation.getState().owners.size).toBeGreaterThan(0);
  });
  it("rejects structural operations during a pending save and keeps failed-delete drafts", async () => {
    let finishSave!: (response: unknown) => void;
    api.saveWorkspaceFile.mockImplementation(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );
    await act(async () => {
      await files.openFile("one.txt");
      files.setDraft("unused");
    });
    await act(async () => {
      void files.saveFile("saved");
    });
    await act(async () => {
      await files.mutations.change({ operation: "delete", path: "one.txt" });
    });
    expect(api.changeWorkspaceEntry).not.toHaveBeenCalled();
    await act(async () => finishSave({ ok: true, result: { sha256: "new", path: "one.txt" } }));
    await act(async () => files.setDraft("keep me"));
    api.changeWorkspaceEntry.mockResolvedValue({
      ok: false,
      error: { message: "permission denied" },
    });
    await act(async () => {
      await files.mutations.change({ operation: "delete", path: "one.txt" });
    });
    expect(files.draft).toBe("keep me");
    expect(files.mutations.error).toContain("permission denied");
  });
  it("stops a batch on collision without claiming already-created entries were rolled back", async () => {
    api.uploadWorkspaceFile.mockResolvedValueOnce({
      ok: false,
      error: { message: "entry exists" },
    });
    await act(async () => {
      await files.mutations.upload("", [
        { path: "folder" },
        { path: "folder/file", file: new File(["x"], "file") },
        { path: "later", file: new File(["y"], "later") },
      ]);
    });
    expect(api.changeWorkspaceEntry).toHaveBeenCalledTimes(1);
    expect(api.uploadWorkspaceFile).toHaveBeenCalledTimes(1);
    expect(files.mutations.uploadProgress).toMatchObject({ completed: 1, total: 3, stopped: true });
    expect(files.mutations.error).toContain("folder/file");
  });
  it("retires in-flight operations on scope changes without changing the new editor", async () => {
    let complete!: (response: unknown) => void;
    api.changeWorkspaceEntry.mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    await act(async () => {
      void files.mutations.change({ operation: "delete", path: "old" });
    });
    await act(async () => root.render(<Harness scope="next" />));
    await act(async () => complete({ ok: true, result: { path: "old" } }));
    expect(files.mutations.busy).toBe(false);
    expect(files.mutations.error).toBe("");
    expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
  });
  it("loads Git only in its visible view and retains unsaved files across view switches", async () => {
    expect(api.fetchWorkspaceChanges).not.toHaveBeenCalled();
    await act(async () => {
      await files.openFile("file.txt");
    });
    await act(async () => files.setDraft("unsaved"));
    await act(async () => files.setMode("changes"));
    expect(api.fetchWorkspaceChanges).toHaveBeenCalledTimes(1);
    await act(async () => files.changes.open("file.txt", "staged"));
    expect(api.fetchWorkspaceDiff).toHaveBeenLastCalledWith(
      "group",
      "scope",
      "/repo",
      "file.txt",
      "staged",
      expect.any(AbortSignal),
    );
    await act(async () => files.setMode("files"));
    expect(files.draft).toBe("unsaved");
    await act(async () => files.refresh());
    expect(api.fetchWorkspaceChanges).toHaveBeenCalledTimes(1);
    await act(async () => files.setMode("changes"));
    expect(api.fetchWorkspaceChanges).toHaveBeenCalledTimes(2);
  });
  it("does not replace a newer Git selection with a delayed old patch", async () => {
    let oldPatch!: (response: unknown) => void;
    api.fetchWorkspaceDiff.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          oldPatch = resolve;
        }),
    );
    await act(async () => files.setMode("changes"));
    await act(async () => files.changes.open("first.txt", "staged"));
    await act(async () => files.changes.open("second.txt", "worktree"));
    await act(async () => oldPatch({ ok: true, result: { patch: "old patch", limited: false } }));
    expect(files.changes.selection?.path).toBe("second.txt");
    expect(files.changes.patch?.patch).toBe("patch");
  });

  it("keeps both drafts when a move would reuse the path of a deleted file with unsaved edits", async () => {
    await act(async () => {
      await files.openFile("source.txt");
    });
    await act(async () => files.setDraft("source draft"));
    await act(async () => {
      await files.openFile("target.txt");
    });
    await act(async () => files.setDraft("target draft"));
    await act(async () => {
      await files.mutations.change({
        operation: "move",
        path: "source.txt",
        destination: "target.txt",
      });
    });
    expect(api.changeWorkspaceEntry).not.toHaveBeenCalled();
    expect(files.mutations.error).toContain("destinationDraft");
    await act(async () => {
      await files.openFile("source.txt");
    });
    expect(files.draft).toBe("source draft");
    await act(async () => {
      await files.openFile("target.txt");
    });
    expect(files.draft).toBe("target draft");
  });
  it("cancels the active upload and does not start later entries", async () => {
    api.uploadWorkspaceFile.mockImplementation(
      (_g, _k, _u, _p, _f, signal: AbortSignal) =>
        new Promise((resolve) =>
          signal.addEventListener("abort", () =>
            resolve({ ok: false, error: { message: "aborted" } }),
          ),
        ),
    );
    let pending!: Promise<void>;
    await act(async () => {
      pending = files.mutations.upload("", [
        { path: "first", file: new File(["a"], "first") },
        { path: "later", file: new File(["b"], "later") },
      ]);
    });
    await act(async () => {
      files.mutations.cancelUpload();
      await pending;
    });
    expect(api.uploadWorkspaceFile).toHaveBeenCalledTimes(1);
    expect(files.mutations.uploadProgress).toMatchObject({ completed: 0, total: 2, stopped: true });
    expect(files.mutations.error).toBe("");
    expect(files.mutations.busy).toBe(false);
  });
});

it.each([
  ["notes.md", "notes.txt", "text/markdown", "text/plain"],
  ["module.txt", "module.ts", "text/plain", "video/vnd.dlna.mpeg-tts"],
])(
  "keeps text accessible after renaming %s to %s in the editor and draft cache",
  async (from, to, oldMime, newMime) => {
    api.fetchWorkspaceFile.mockImplementation(async (_g, path) => ({
      ok: true,
      result: { ...diskFile(path), mime_type: path === from ? oldMime : "text/plain" },
    }));
    api.changeWorkspaceEntry.mockResolvedValue({
      ok: true,
      result: { path: from, destination: to, mime_type: newMime },
    });
    await act(async () => {
      await files.openFile(from);
    });
    await act(async () => files.setDraft("unsaved text"));
    await act(async () => {
      await files.mutations.change({ operation: "move", path: from, destination: to });
    });
    expect(files.file?.mime_type).toBe(newMime);
    expect(workspacePreviewKind(files.file!)).toBe("text");
    expect(files.draft).toBe("unsaved text");
    await act(async () => {
      await files.openFile("other.txt");
    });
    await act(async () => {
      await files.openFile(to);
    });
    expect(files.file?.mime_type).toBe(newMime);
    expect(workspacePreviewKind(files.file!)).toBe("text");
    expect(files.draft).toBe("unsaved text");
    expect(files.file?.sha256).toBe("sha");
  },
);
