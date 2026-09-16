/** Convert a pasted path to the workspace-relative form accepted by the file API. */
export function workspaceRelativePath(input: string, root: string): string | null {
  let path = input.trim();
  if (
    (path.startsWith('"') && path.endsWith('"')) ||
    (path.startsWith("'") && path.endsWith("'"))
  ) {
    path = path.slice(1, -1);
  }
  if (!path) return null;
  const windows = /^[A-Za-z]:[\\/]|^\\\\/.test(root);
  const normalize = (value: string) =>
    windows
      ? value
          .replace(/^\\\\\?\\UNC\\/i, "//")
          .replace(/^\\\\\?\\/, "")
          .replace(/\\/g, "/")
      : value;
  path = normalize(path);
  const base = normalize(root).replace(/\/+$/, "");
  const compare = (value: string) => (windows ? value.toLowerCase() : value);
  if (path.startsWith("/") || /^[A-Za-z]:/.test(path)) {
    if (!base && root === "/") path = path.slice(1);
    else if (base && compare(path.replace(/\/+$/, "")) === compare(base)) path = "";
    else if (base && compare(path).startsWith(compare(base) + "/"))
      path = path.slice(base.length + 1);
    else return null;
  }
  if (/^[A-Za-z][A-Za-z\d+.-]*:\/\//.test(path)) return null;
  const segments = path.split("/");
  if (segments.includes("..")) return null;
  path = segments.filter((part) => part && part !== ".").join("/");
  return path;
}

export function workspaceAbsolutePath(root: string, path: string): string {
  if (!root) return path;
  const windows = /^[A-Za-z]:[\\/]|^\\\\/.test(root);
  const separator = windows ? "\\" : "/";
  return (
    root.replace(windows ? /[\\/]+$/ : /\/+$/, "") +
    separator +
    (windows ? path.replace(/\//g, "\\") : path)
  );
}
