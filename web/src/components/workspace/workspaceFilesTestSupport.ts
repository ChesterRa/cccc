// @vitest-environment happy-dom
import { act, createElement } from "react";
import { Harness, type HarnessProps } from "./WorkspaceFilesTestHarness";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, vi } from "vite-plus/test";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
    i18n: { language: "en" },
  }),
}));

const { resolveWorkspacePath, fetchWorkspaceListing, fetchWorkspaceFile, saveWorkspaceFile } =
  vi.hoisted(() => ({
    resolveWorkspacePath: vi.fn(),
    fetchWorkspaceListing: vi.fn(),
    fetchWorkspaceFile: vi.fn(),
    saveWorkspaceFile: vi.fn(),
  }));

vi.mock("../../services/api", () => ({
  resolveWorkspacePath,
  fetchWorkspaceListing,
  fetchWorkspaceFile,
  saveWorkspaceFile,
}));

const ROOT_ITEMS = [
  { name: "src", path: "src", is_dir: true, git_dirty_descendant: true },
  { name: "README.txt", path: "README.txt", is_dir: false, git_status: "modified" as const },
];

function listing(path: string, items: unknown[]) {
  return {
    ok: true as const,
    result: {
      scope_key: "scope-a",
      scope_url: "/repo",
      root_path: "/repo",
      path,
      parent: path ? "" : null,
      items,
    },
  };
}

let root: Root;
let host: HTMLDivElement;

async function render(props: HarnessProps) {
  await act(async () => root.render(createElement(Harness, props)));
  await act(async () => {
    await Promise.resolve();
  });
}

async function mount(props: HarnessProps = {}) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(createElement(Harness, props)));
  // Let the root listing request settle before the test inspects the tree.
  await act(async () => {
    await Promise.resolve();
  });
}

function panel(): HTMLElement {
  return host.querySelector<HTMLElement>('[data-testid="side-panel"]')!;
}

function mainArea(): HTMLElement {
  return host.querySelector<HTMLElement>('[data-testid="main-area"]')!;
}

function rowByName(name: string): HTMLButtonElement {
  const row = [...panel().querySelectorAll<HTMLButtonElement>('[role="treeitem"]')].find((node) =>
    node.textContent?.includes(name),
  );
  if (!row) throw new Error(`no tree row for ${name}, saw: ${panel().textContent}`);
  return row;
}

function saveButton(): HTMLButtonElement {
  const node = [...mainArea().querySelectorAll("button")].find(
    (item) => item.getAttribute("title") === "Save",
  );
  if (!node) throw new Error(`no Save button, saw: ${mainArea().textContent}`);
  return node;
}

async function edit(value: string) {
  const editor = mainArea().querySelector("textarea");
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set?.call(
      editor,
      value,
    );
    editor?.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function click(node: HTMLElement) {
  await act(async () => {
    node.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  localStorage.removeItem("cccc-workspace-show-ignored");
  vi.clearAllMocks();
});

export {
  ROOT_ITEMS,
  listing,
  render,
  mount,
  panel,
  mainArea,
  rowByName,
  saveButton,
  edit,
  click,
  host,
  resolveWorkspacePath,
  fetchWorkspaceListing,
  fetchWorkspaceFile,
  saveWorkspaceFile,
};
