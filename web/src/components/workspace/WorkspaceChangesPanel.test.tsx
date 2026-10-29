// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import type { WorkspaceChanges } from "../../services/api/workspace";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";
import { WorkspaceChangesPanel } from "./WorkspaceChangesPanel";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let root: Root | null = null;
const host = document.createElement("div");

afterEach(() => {
  act(() => root?.unmount());
  root = null;
});

function render(status: WorkspaceChanges) {
  const files = {
    changes: { loading: false, error: "", status, selection: null, open: vi.fn() },
    setMode: vi.fn(),
    locatePath: vi.fn(),
  } as unknown as WorkspaceFilesController;
  root ??= createRoot(host);
  act(() => root!.render(<WorkspaceChangesPanel files={files} />));
}

it("does not call an incomplete list without entries clean", () => {
  // A workspace that is itself a new folder too large to list arrives with no entries.
  render({ repository: true, branch: "main", entries: [], limited: true });
  expect(host.querySelector('[role="status"]')?.textContent).toBe("workspaceGit.limited");
  expect(host.textContent).not.toContain("workspaceGit.clean");
  render({ repository: true, branch: "main", entries: [], limited: false });
  expect(host.textContent).toContain("workspaceGit.clean");
});
