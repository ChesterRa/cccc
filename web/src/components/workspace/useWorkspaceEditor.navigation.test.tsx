// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vite-plus/test";
import { useWorkspaceEditor } from "./useWorkspaceEditor";
import { WorkspaceFileViewer } from "./WorkspaceFileViewer";
import type { WorkspaceFile } from "../../types";

const { fetchWorkspaceFile } = vi.hoisted(() => ({ fetchWorkspaceFile: vi.fn() }));
vi.mock("../../services/api", () => ({ fetchWorkspaceFile }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let editor: ReturnType<typeof useWorkspaceEditor>;
let host: HTMLDivElement, root: Root;
const noop = () => undefined;
const file = (path: string): WorkspaceFile => ({
  path,
  scope_key: "scope-a",
  scope_url: "/repo",
  content: "# Overview\n\n[Same](#details) [Other](other.md#details)\n\n## Details\n\nText",
  mime_type: "text/markdown",
  truncated: false,
  binary: false,
  sha256: "disk",
  bytes: 100,
});
function Harness({ groupId = "g_a", scopeKey = "scope-a", scopeUrl = "/repo" }) {
  editor = useWorkspaceEditor(groupId, scopeKey, scopeUrl, noop, noop, noop);
  return editor.file ? (
    <WorkspaceFileViewer
      groupId={groupId}
      file={editor.file}
      navigation={editor.navigation}
      draft={editor.draft}
      setDraft={editor.setDraft}
      isDark={false}
      readOnly={false}
      saving={editor.saving}
      error={editor.fileError}
      conflict={editor.conflict}
      onClose={editor.closeFile}
      onSave={editor.saveFile}
      onReload={noop}
      onAttach={noop}
      onOpenFile={editor.openFile}
    />
  ) : null;
}
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  fetchWorkspaceFile.mockImplementation(async (_group: string, path: string) => ({
    ok: true,
    result: file(path),
  }));
  await act(async () => {
    await import("../MarkdownRenderer");
    root.render(<Harness />);
  });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

it("carries fragments through fresh and cached navigation, and cancels a pending read on a same-file anchor", async () => {
  await act(async () => editor.openFile("readme.md", { fragment: "#details" }));
  expect(editor.navigation?.fragment).toBe("#details");
  const firstNavigation = editor.navigation;
  await act(async () => editor.openFile("readme.md"));
  expect(editor.navigation).toBe(firstNavigation);
  await act(async () => editor.setDraft(editor.draft + "\nunsaved"));
  await act(async () => editor.openFile("other.md"));
  expect(editor.navigation).toBeNull();
  fetchWorkspaceFile.mockClear();
  await act(async () => editor.openFile("readme.md", { fragment: "#overview" }));
  expect(fetchWorkspaceFile).not.toHaveBeenCalled();
  expect(editor.draft).toContain("unsaved");
  expect(editor.navigation?.fragment).toBe("#overview");
  let finish!: (result: unknown) => void;
  fetchWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let pending!: ReturnType<typeof editor.openFile>;
  await act(async () => {
    pending = editor.openFile("delayed.md", { fragment: "#elsewhere" });
  });
  expect(editor.navigation).toBeNull();
  await act(async () => editor.openFile("readme.md", { fragment: "#details" }));
  const navigation = editor.navigation;
  await act(async () => {
    finish({ ok: true, result: file("delayed.md") });
    await pending;
  });
  expect(editor.file?.path).toBe("readme.md");
  expect(editor.navigation).toBe(navigation);
});

it("retires fragment intent on failures, scope changes and close, ignoring late responses", async () => {
  await act(async () => editor.openFile("readme.md", { fragment: "#details" }));
  fetchWorkspaceFile.mockResolvedValueOnce({ ok: false, error: { message: "denied" } });
  await act(async () => editor.openFile("denied.md", { fragment: "#details" }));
  expect(editor.navigation).toBeNull();
  expect(editor.file?.path).toBe("readme.md");
  let finish!: (result: unknown) => void;
  fetchWorkspaceFile.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  let pending!: ReturnType<typeof editor.openFile>;
  await act(async () => {
    pending = editor.openFile("next.md", { fragment: "#details" });
  });
  await act(async () => root.render(<Harness scopeKey="scope-b" scopeUrl="/elsewhere" />));
  await act(async () => {
    finish({ ok: true, result: file("next.md") });
    await pending;
  });
  expect(editor.file).toBeNull();
  expect(editor.navigation).toBeNull();
  await act(async () => editor.closeFile());
  expect(editor.navigation).toBeNull();
});

it("positions once after Markdown rendering, repeats explicit anchor clicks, and opens a source view at its requested section", async () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return {
        top:
          this.id === "details"
            ? 600 - (host.querySelector<HTMLElement>(".overflow-auto")?.scrollTop || 0)
            : 0,
        left: 0,
        width: 600,
        height: 400,
        right: 600,
        bottom: 400,
        x: 0,
        y: 0,
        toJSON: noop,
      };
    },
  );
  await act(async () => editor.openFile("readme.md", { fragment: "#details" }));
  let viewport = host.querySelector<HTMLElement>(".overflow-auto")!;
  expect(viewport.scrollTop).toBe(600);
  viewport.scrollTop = 30;
  await act(async () => root.render(<Harness />));
  expect(viewport.scrollTop).toBe(30);
  await act(async () => host.querySelector<HTMLAnchorElement>('a[href="#details"]')!.click());
  expect(viewport.scrollTop).toBe(600);
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[aria-label="workspaceViewSource"]')!.click(),
  );
  expect(host.querySelector("textarea")).not.toBeNull();
  await act(async () => editor.openFile("readme.md", { fragment: "#details" }));
  expect(host.querySelector("textarea")).toBeNull();
  viewport = host.querySelector<HTMLElement>(".overflow-auto")!;
  expect(viewport.scrollTop).toBe(600);
});
