import type { WorkspaceEntry } from "../../types";

/** Directory listings keyed by workspace-relative path; the root is the empty string. */
export type DirectoryState = { items: WorkspaceEntry[]; loading: boolean; error: string };

export type TreeState = { directories: Record<string, DirectoryState>; expanded: string[] };

export type TreeNode = {
  /** Position in the displayed tree; symlink branches may share an entry.path. */
  key: string;
  entry: WorkspaceEntry;
  depth: number;
  expanded: boolean;
  loading: boolean;
  error?: string;
};

export const ROOT_PATH = "";

function directoryAt(state: TreeState, path: string): DirectoryState | undefined {
  return Object.prototype.hasOwnProperty.call(state.directories, path)
    ? state.directories[path]
    : undefined;
}

export function emptyTreeState(): TreeState {
  return { directories: {}, expanded: [] };
}

/** `"a/b/c"` -> `["", "a", "a/b"]`, the directories that must be open to reveal the path. */
export function ancestorsOf(path: string): string[] {
  const parts = path.split("/").filter(Boolean);
  const ancestors = [ROOT_PATH];
  let current = "";
  for (const part of parts.slice(0, -1)) {
    current = current ? `${current}/${part}` : part;
    ancestors.push(current);
  }
  return ancestors;
}

export function isExpanded(state: TreeState, path: string): boolean {
  return state.expanded.includes(path);
}

export function toggleExpanded(state: TreeState, path: string): TreeState {
  const expanded = state.expanded.includes(path)
    ? state.expanded.filter((value) => value !== path)
    : [...state.expanded, path];
  return { ...state, expanded };
}

export function expandPaths(state: TreeState, paths: string[]): TreeState {
  const missing = paths.filter(
    (path, index) => !state.expanded.includes(path) && paths.indexOf(path) === index,
  );
  return missing.length ? { ...state, expanded: [...state.expanded, ...missing] } : state;
}

export function setDirectory(
  state: TreeState,
  path: string,
  directory: Partial<DirectoryState>,
): TreeState {
  const previous = directoryAt(state, path) || { items: [], loading: false, error: "" };
  return { ...state, directories: { ...state.directories, [path]: { ...previous, ...directory } } };
}

/**
 * Forgets every cached listing so the next render refetches. Expansion is kept, so a refresh
 * does not collapse the tree the user just opened.
 */
export function invalidateDirectories(state: TreeState): TreeState {
  return { ...state, directories: {} };
}

/**
 * Depth-first rows for rendering; unexpanded and unloaded directories contribute nothing.
 *
 * `ancestors` breaks directory cycles. A self-referential symlink (`a/b -> a`) resolves back to
 * its own parent, so the listing for `a/b` contains an entry whose path is again `a/b`; without
 * this guard the recursion never terminates and the tab locks up.
 */
export function flattenTree(
  state: TreeState,
  path: string = ROOT_PATH,
  depth = 0,
  ancestors: ReadonlySet<string> = new Set(),
  branchPath: string = path,
): TreeNode[] {
  const directory = directoryAt(state, path);
  if (!directory) return [];
  const rows: TreeNode[] = [];
  for (const entry of directory.items) {
    const key = branchPath ? `${branchPath}/${entry.name}` : entry.name;
    const cyclic = entry.path === path || ancestors.has(entry.path);
    const expanded = entry.is_dir && !entry.unavailable && !cyclic && isExpanded(state, entry.path);
    const child = directoryAt(state, entry.path);
    rows.push({
      key,
      entry,
      depth,
      expanded,
      loading: expanded && (!child || child.loading),
      ...(expanded && child?.error ? { error: child.error } : {}),
    });
    if (expanded && child && !child.loading && !child.error) {
      rows.push(
        ...flattenTree(
          state,
          entry.path,
          depth + 1,
          new Set([...ancestors, path, entry.path]),
          key,
        ),
      );
    }
  }
  return rows;
}

/** Directories that are expanded but have no listing yet, so the panel knows what to fetch. */
export function pendingDirectories(state: TreeState): string[] {
  return [ROOT_PATH, ...state.expanded].filter(
    (path, index, all) => all.indexOf(path) === index && !directoryAt(state, path),
  );
}

export function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}
