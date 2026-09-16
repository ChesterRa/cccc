// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { WorkspaceFileViewer } from "./WorkspaceFileViewer";
import { useWorkspaceFiles, type WorkspaceFilesController } from "./useWorkspaceFiles";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) => opts?.defaultValue || key,
  }),
}));
const { fetchWorkspaceFile, saveWorkspaceFile } = vi.hoisted(() => ({
  fetchWorkspaceFile: vi.fn(),
  saveWorkspaceFile: vi.fn(),
}));
vi.mock("../../services/api", () => ({ fetchWorkspaceFile, saveWorkspaceFile }));
let files: WorkspaceFilesController;
let root: Root;
let host: HTMLDivElement;
function Harness({
  visible = true,
  groupId = "group-1",
  scopeKey = "scope-a",
  scopeUrl = "/repo",
}) {
  files = useWorkspaceFiles(groupId, false, scopeKey, scopeUrl);
  return visible && files.file ? (
    <WorkspaceFileViewer
      groupId={groupId}
      file={files.file}
      draft={files.draft}
      setDraft={files.setDraft}
      isDark={false}
      readOnly={false}
      saving={files.saving}
      loading={files.fileLoading}
      reloadVersion={files.reloadVersion}
      error={files.fileError}
      conflict={files.conflict}
      onClose={files.closeFile}
      onSave={files.saveFile}
      onReload={() => void files.openFile(files.file!.path, { reload: true })}
      onAttach={() => undefined}
    />
  ) : null;
}
async function render(
  visible = true,
  groupId = "group-1",
  scopeKey = "scope-a",
  scopeUrl = "/repo",
) {
  await act(async () =>
    root.render(
      <Harness visible={visible} groupId={groupId} scopeKey={scopeKey} scopeUrl={scopeUrl} />,
    ),
  );
}
async function setup() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  fetchWorkspaceFile.mockResolvedValue({
    ok: true,
    result: {
      scope_key: "scope-a",
      scope_url: "/repo",
      path: "README.txt",
      content: "disk",
      sha256: "old",
      bytes: 4,
      mime_type: "text/plain",
      binary: false,
      truncated: false,
    },
  });
  await render();
  await act(async () => {
    await files.openFile("README.txt");
  });
}
async function edit(value: string) {
  await act(async () => {
    const editor = host.querySelector("textarea")!;
    Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")!.set!.call(
      editor,
      value,
    );
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
afterEach(async () => {
  if (root) await act(async () => root.unmount());
  host?.remove();
  vi.resetAllMocks();
});
it.each([
  ["CRLF", "one\r\ntwo\r\nthree\r\n"],
  ["LF", "one\ntwo\nthree\n"],
  ["CR", "one\rtwo\rthree\r"],
  ["mixed", "one\r\ntwo\nthree\rend"],
])("preserves %s endings across edits, undo, remount and keyboard save", async (_, content) => {
  await setup();
  fetchWorkspaceFile.mockResolvedValue({
    ok: true,
    result: { ...files.file, content, bytes: content.length },
  });
  await act(async () => {
    await files.openFile("README.txt", { reload: true });
  });
  const display = content.replace(/\r\n?/g, "\n");
  expect(host.querySelector("textarea")?.value).toBe(display);
  expect(host.querySelector<HTMLButtonElement>('button[title="Save"]')!.disabled).toBe(true);
  await edit(display.replace("two", "tXwo"));
  expect(files.draft).toBe(content.replace("two", "tXwo"));
  await edit(display);
  expect(files.draft).toBe(content);
  expect(host.querySelector<HTMLButtonElement>('button[title="Save"]')!.disabled).toBe(true);

  await edit(display.replace("three", "inserted\nthree"));
  const newline = content.match(/\r\n|\r|\n/)![0];
  const saved = content.replace("three", `inserted${newline}three`);
  expect(files.draft).toBe(saved);
  await render(false);
  await render();
  expect(files.draft).toBe(saved);
  saveWorkspaceFile.mockResolvedValue({ ok: true, result: { sha256: "saved" } });
  await act(async () => {
    host
      .querySelector("textarea")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true }));
  });
  expect(saveWorkspaceFile).toHaveBeenCalledWith(
    "group-1",
    "README.txt",
    saved,
    "old",
    "scope-a",
    "/repo",
  );
  expect(files.file?.content).toBe(saved);
  expect(host.querySelector<HTMLButtonElement>('button[title="Save"]')!.disabled).toBe(true);
});
it("preserves unsaved edits across repeated viewer unmounts and saves the restored draft", async () => {
  await setup();
  await edit("unsaved work");
  for (let i = 0; i < 2; i++) {
    await render(false);
    expect(host.querySelector("textarea")).toBeNull();
    await render();
    expect(host.querySelector("textarea")?.value).toBe("unsaved work");
  }
  saveWorkspaceFile.mockResolvedValue({ ok: true, result: { sha256: "saved" } });
  await act(async () => {
    host.querySelector<HTMLButtonElement>('button[title="Save"]')!.click();
  });
  expect(saveWorkspaceFile).toHaveBeenCalledWith(
    "group-1",
    "README.txt",
    "unsaved work",
    "old",
    "scope-a",
    "/repo",
  );
  expect(host.querySelector<HTMLButtonElement>('button[title="Save"]')!.disabled).toBe(true);
});
it("keeps typing made during a save even when the viewer is hidden before the response", async () => {
  await setup();
  await edit("submitted");
  let finish!: (value: unknown) => void;
  saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = files.saveFile(files.draft);
  });
  await edit("newer typing");
  await render(false);
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved" } });
    await saving;
  });
  await render();
  expect(host.querySelector("textarea")?.value).toBe("newer typing");
  expect(files.file?.content).toBe("submitted");
  expect(host.querySelector<HTMLButtonElement>('button[title="Save"]')!.disabled).toBe(false);
});
it("adopts disk content on explicit reload and clears the draft when changing groups", async () => {
  await setup();
  await edit("unsaved");
  await act(async () => {
    await files.openFile("README.txt", { reload: true });
  });
  expect(host.querySelector("textarea")?.value).toBe("disk");
  await edit("group one draft");
  await render(true, "group-2");
  expect(files.draft).toBe("");
  expect(host.querySelector("textarea")).toBeNull();
});

it("ignores repeat selection and restores drafts after switching or closing files", async () => {
  await setup();
  await edit("unsaved README");
  await act(async () => {
    await files.openFile("README.txt");
  });
  expect(fetchWorkspaceFile).toHaveBeenCalledTimes(1);
  expect(host.querySelector("textarea")?.value).toBe("unsaved README");
  fetchWorkspaceFile.mockResolvedValue({
    ok: true,
    result: { ...files.file, path: "LICENSE", content: "license" },
  });
  await act(async () => {
    await files.openFile("LICENSE");
  });
  await edit("unsaved LICENSE");
  await act(async () => {
    await files.openFile("README.txt");
  });
  expect(host.querySelector("textarea")?.value).toBe("unsaved README");
  await act(async () => {
    files.closeFile();
  });
  await act(async () => {
    await files.openFile("LICENSE");
  });
  expect(host.querySelector("textarea")?.value).toBe("unsaved LICENSE");
  expect(fetchWorkspaceFile).toHaveBeenCalledTimes(2);
});
it("keeps the current draft when another file or an explicit reload fails", async () => {
  await setup();
  await edit("keep me");
  fetchWorkspaceFile.mockResolvedValue({ ok: false, error: { message: "unavailable" } });
  await act(async () => {
    await files.openFile("missing.txt");
  });
  expect(host.querySelector("textarea")?.value).toBe("keep me");
  await act(async () => {
    await files.openFile("README.txt", { reload: true });
  });
  expect(host.querySelector("textarea")?.value).toBe("keep me");
  expect(files.fileError).toBe("unavailable");
});
it("cancels a pending navigation when the current file is selected again", async () => {
  await setup();
  await edit("still editing");
  let finish!: (value: unknown) => void;
  fetchWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let opening!: ReturnType<typeof files.openFile>;
  await act(async () => {
    opening = files.openFile("LICENSE");
  });
  await act(async () => {
    await files.openFile("README.txt");
  });
  await act(async () => {
    finish({ ok: true, result: { ...files.file, path: "LICENSE", content: "license" } });
    await opening;
  });
  expect(files.file?.path).toBe("README.txt");
  expect(host.querySelector("textarea")?.value).toBe("still editing");
});

it("restores an unsaved draft when reopening an internal symlink", async () => {
  await setup();
  await edit("keep target draft");
  await act(async () => files.closeFile());
  await act(async () => files.openFile("README-link.txt"));
  expect(files.file?.path).toBe("README.txt");
  expect(host.querySelector("textarea")?.value).toBe("keep target draft");
});

it("preserves an edit reverted to the old content while a save is pending", async () => {
  await setup();
  await edit("submitted");
  let finish!: (value: unknown) => void;
  saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = files.saveFile(files.draft);
  });
  await edit("disk");
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved" } });
    await saving;
  });
  fetchWorkspaceFile.mockResolvedValue({
    ok: true,
    result: { ...files.file, content: "submitted" },
  });
  await act(async () => files.closeFile());
  await act(async () => files.openFile("README.txt"));
  expect(host.querySelector("textarea")?.value).toBe("disk");
});

it("finishes a save after leaving and returning to its file without a stale digest", async () => {
  await setup();
  await edit("submitted");
  let finish!: (value: unknown) => void;
  saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = files.saveFile(files.draft);
  });
  await act(async () => files.closeFile());
  await act(async () => files.openFile("README.txt"));
  expect(files.saving).toBe(true);
  await edit("disk");
  await act(async () => files.closeFile());
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved" } });
    await saving;
  });
  await act(async () => files.openFile("README.txt"));
  expect(files.draft).toBe("disk");
  expect(files.file?.sha256).toBe("saved");
  expect(files.saving).toBe(false);
});

it("retires drafts and pending opens when the same Group changes scope or location", async () => {
  await setup();
  await edit("old workspace draft");
  const oldFile = files.file!;
  let finish!: (value: unknown) => void;
  fetchWorkspaceFile.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let opening!: ReturnType<typeof files.openFile>;
  await act(async () => {
    opening = files.openFile("LICENSE");
  });
  await render(true, "group-1", "scope-b", "/other");
  expect(files.file).toBeNull();
  expect(files.draft).toBe("");
  await act(async () => {
    finish({ ok: true, result: { ...oldFile, path: "LICENSE" } });
    await opening;
  });
  expect(files.file).toBeNull();
  fetchWorkspaceFile.mockResolvedValue({
    ok: true,
    result: { ...oldFile, scope_key: "scope-b", scope_url: "/other", content: "new workspace" },
  });
  await act(async () => {
    await files.openFile("README.txt");
  });
  expect(files.draft).toBe("new workspace");
  await edit("new edit");
  await render(true, "group-1", "scope-b", "/relocated");
  expect(files.file).toBeNull();
  expect(files.draft).toBe("");
});

it("ignores a save completion after a scope switch and sends the opened scope identity", async () => {
  await setup();
  await edit("old scope edit");
  let finish!: (value: unknown) => void;
  saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = files.saveFile(files.draft);
  });
  expect(saveWorkspaceFile).toHaveBeenCalledWith(
    "group-1",
    "README.txt",
    "old scope edit",
    "old",
    "scope-a",
    "/repo",
  );
  await render(true, "group-1", "scope-b", "/other");
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved" } });
    await saving;
  });
  expect(files.file).toBeNull();
  expect(files.saving).toBe(false);
});

it.each(["file", "close", "scope", "return"])(
  "keeps failed-save feedback with its file after %s navigation",
  async (next) => {
    await setup();
    await edit("submitted");
    const original = files.file!;
    let finish!: (value: unknown) => void;
    saveWorkspaceFile.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    let saving!: Promise<boolean>;
    await act(async () => {
      saving = files.saveFile(files.draft);
    });
    if (next === "file") {
      fetchWorkspaceFile.mockResolvedValueOnce({
        ok: true,
        result: { ...original, path: "LICENSE" },
      });
      await act(async () => files.openFile("LICENSE"));
    } else if (next === "scope") {
      await render(true, "group-1", "scope-b", "/other");
      fetchWorkspaceFile.mockResolvedValueOnce({
        ok: true,
        result: { ...original, scope_key: "scope-b", scope_url: "/other" },
      });
      await act(async () => files.openFile("README.txt"));
    } else {
      await act(async () => files.closeFile());
      if (next === "return") await act(async () => files.openFile("README.txt"));
    }
    await act(async () => {
      finish({
        ok: false,
        error: { code: "workspace_write_conflict", message: "File changed on disk" },
      });
      await saving;
    });
    expect(files.fileError).toBe(next === "return" ? "File changed on disk" : "");
    expect(files.conflict).toBe(next === "return");
    expect(files.saving).toBe(false);
    if (next === "return") expect(files.draft).toBe("submitted");
  },
);

it("requires explicit confirmation before reloading a dirty file, and retains it if the read fails", async () => {
  await setup();
  await edit("unsaved");
  const reload = host.querySelector<HTMLButtonElement>('[title="Reload file"]')!;
  await act(async () => reload.click());
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(fetchWorkspaceFile).toHaveBeenCalledTimes(1);
  const cancel = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
    (button) => button.textContent === "Cancel",
  )!;
  await act(async () => cancel.click());
  expect(files.draft).toBe("unsaved");
  await act(async () => reload.click());
  fetchWorkspaceFile.mockResolvedValue({ ok: false, error: { message: "offline" } });
  const discard = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
    (button) => button.textContent === "Discard and reload",
  )!;
  await act(async () => discard.click());
  expect(files.draft).toBe("unsaved");
  expect(files.fileError).toBe("offline");
});

it("preserves typing made while a confirmed reload is in flight", async () => {
  await setup();
  await edit("discard this");
  const baseline = files.file!;
  let finish!: (value: unknown) => void;
  fetchWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let pending!: ReturnType<typeof files.openFile>;
  await act(async () => {
    pending = files.openFile("README.txt", { reload: true });
  });
  await edit("new typing");
  await act(async () => {
    finish({ ok: true, result: { ...baseline, content: "new disk content", sha256: "new" } });
    await pending;
  });
  expect(files.draft).toBe("new typing");
  // Retain the original digest so a later save detects the external modification.
  expect(files.file?.sha256).toBe("old");
});

it("does not reload over an in-flight save", async () => {
  await setup();
  await edit("submitted");
  let finish!: (value: unknown) => void;
  saveWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let pending!: Promise<boolean>;
  await act(async () => {
    pending = files.saveFile(files.draft);
  });
  expect(host.querySelector<HTMLButtonElement>('[title="Reload file"]')?.disabled).toBe(true);
  await act(async () => {
    await files.openFile("README.txt", { reload: true });
  });
  expect(fetchWorkspaceFile).toHaveBeenCalledTimes(1);
  await act(async () => {
    finish({ ok: true, result: { sha256: "saved" } });
    await pending;
  });
  expect(files.file?.content).toBe("submitted");
});
