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
function Harness({ visible = true, groupId = "group-1" }) {
  files = useWorkspaceFiles(groupId, false);
  return visible && files.file ? (
    <WorkspaceFileViewer
      file={files.file}
      draft={files.draft}
      setDraft={files.setDraft}
      isDark={false}
      readOnly={false}
      saving={files.saving}
      error={files.fileError}
      conflict={files.conflict}
      onClose={files.closeFile}
      onSave={files.saveFile}
      onReload={() => void files.openFile(files.file!.path, { reload: true })}
      onAttach={() => undefined}
    />
  ) : null;
}
async function render(visible = true, groupId = "group-1") {
  await act(async () => root.render(<Harness visible={visible} groupId={groupId} />));
}
async function setup() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  fetchWorkspaceFile.mockResolvedValue({
    ok: true,
    result: {
      path: "README.md",
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
    await files.openFile("README.md");
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
  expect(saveWorkspaceFile).toHaveBeenCalledWith("group-1", "README.md", "unsaved work", "old");
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
    await files.openFile("README.md", { reload: true });
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
    await files.openFile("README.md");
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
    await files.openFile("README.md");
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
    await files.openFile("README.md", { reload: true });
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
  let opening!: Promise<void>;
  await act(async () => {
    opening = files.openFile("LICENSE");
  });
  await act(async () => {
    await files.openFile("README.md");
  });
  await act(async () => {
    finish({ ok: true, result: { ...files.file, path: "LICENSE", content: "license" } });
    await opening;
  });
  expect(files.file?.path).toBe("README.md");
  expect(host.querySelector("textarea")?.value).toBe("still editing");
});
