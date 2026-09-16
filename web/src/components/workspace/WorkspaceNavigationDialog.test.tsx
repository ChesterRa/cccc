// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vite-plus/test";
import { WorkspaceNavigationDialog } from "./WorkspaceNavigationDialog";
import { useWorkspaceEditor } from "./useWorkspaceEditor";
import {
  requestWorkspaceNavigation,
  finishWorkspaceNavigation,
  useWorkspaceNavigation,
} from "../../stores/workspaceNavigation";

const api = vi.hoisted(() => ({ fetchWorkspaceFile: vi.fn(), saveWorkspaceFile: vi.fn() }));
vi.mock("../../services/api", () => api);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let root: Root, host: HTMLDivElement;
let editor: ReturnType<typeof useWorkspaceEditor>;
const noop = () => {};
function Probe({ groupId = "a", scopeKey = "scope" }) {
  editor = useWorkspaceEditor(groupId, scopeKey, "/repo", noop, noop, noop);
  return <WorkspaceNavigationDialog />;
}
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  api.fetchWorkspaceFile.mockImplementation(async (_group, path) => ({
    ok: true,
    result: {
      scope_key: "scope",
      scope_url: "/repo",
      path,
      content: "original",
      sha256: "old",
      bytes: 8,
      binary: false,
      truncated: false,
      mime_type: "text/plain",
    },
  }));
  api.saveWorkspaceFile.mockResolvedValue({ ok: true, result: { sha256: "saved" } });
  await act(async () => root.render(<Probe />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.clearAllMocks();
});
const unloading = () => {
  const e = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(e);
  return e.defaultPrevented;
};

it("protects hidden drafts, cancels navigation, then accepts explicit discard without saving", async () => {
  const navigate = vi.fn();
  expect(unloading()).toBe(false);
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("edited"));
  await act(async () => editor.openFile("b.txt"));
  await act(async () => editor.closeFile());
  expect(unloading()).toBe(true);
  await act(async () => requestWorkspaceNavigation(navigate));
  expect(navigate).not.toHaveBeenCalled();
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(document.activeElement?.textContent).toBe("workspaceUnsaved.stay");
  await act(async () => finishWorkspaceNavigation(false));
  await act(async () => editor.openFile("a.txt"));
  expect(editor.draft).toBe("edited");
  expect(navigate).not.toHaveBeenCalled();
  await act(async () => requestWorkspaceNavigation(navigate));
  await act(async () => finishWorkspaceNavigation(true));
  expect(navigate).toHaveBeenCalledOnce();
  expect(api.saveWorkspaceFile).not.toHaveBeenCalled();
});

it("removes protection only when all drafts and outstanding saves are settled", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("edited a"));
  await act(async () => editor.openFile("b.txt"));
  await act(async () => editor.setDraft("edited b"));
  await act(async () => editor.saveFile("edited b"));
  expect(unloading()).toBe(true);
  await act(async () => editor.openFile("a.txt"));
  let finish!: (value: unknown) => void;
  api.saveWorkspaceFile.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = editor.saveFile("edited a");
  });
  expect(unloading()).toBe(true);
  await act(async () => {
    finish({ ok: true, result: { sha256: "new" } });
    await saving;
  });
  expect(unloading()).toBe(false);
  const navigate = vi.fn();
  requestWorkspaceNavigation(navigate);
  expect(navigate).toHaveBeenCalledOnce();
});

it("retires pending navigation on an authoritative scope change instead of applying an old action", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("edited"));
  const navigate = vi.fn();
  await act(async () => requestWorkspaceNavigation(navigate));
  await act(async () => root.render(<Probe scopeKey="replacement" />));
  expect(editor.file).toBeNull();
  expect(unloading()).toBe(false);
  expect(useWorkspaceNavigation.getState().pending).toBeNull();
  finishWorkspaceNavigation(true);
  expect(navigate).not.toHaveBeenCalled();
});

it("does not resurrect dirty ownership when an old editor save finishes after unmount", async () => {
  await act(async () => editor.openFile("a.txt"));
  await act(async () => editor.setDraft("edited"));
  let finish!: (value: unknown) => void;
  api.saveWorkspaceFile.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  let saving!: Promise<boolean>;
  await act(async () => {
    saving = editor.saveFile("edited");
  });
  await act(async () => root.render(null));
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
  await act(async () => {
    finish({ ok: false, error: { code: "network", message: "offline" } });
    await saving;
  });
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
});
