import { workspaceRelativePath } from "../components/workspace/workspacePath";
import { findMessageLinkRanges } from "./mentionHighlight";
import { fileKindForName } from "./messageAttachments";

export type MessagePathRange = { start: number; end: number; path: string };

const isWindowsRoot = (root: string) => /^[A-Za-z]:[\\/]|^\\\\/.test(root);

// Files also shows a page as static HTML, which a message attachment does not.
const opensInFiles = (name: string) => !!fileKindForName(name) || /\.html?$/iu.test(name);

/** The workspace file a Markdown link points to, or null for any other address. */
export function messageHrefPath(href: string, root: string): string | null {
  // Two letters or more before a colon make a scheme; one is a Windows drive.
  if (!root || !href || /^(?:#|\/\/|[A-Za-z][A-Za-z\d+.-]+:)/u.test(href)) return null;
  const address = href.split(/[?#]/u)[0].replace(/(?::\d+){1,2}$/u, "");
  let decoded: string;
  try {
    decoded = decodeURIComponent(address);
  } catch {
    return null;
  }
  // ~ means a home directory here, not a folder in the workspace.
  const path = decoded.startsWith("~") ? null : workspaceRelativePath(decoded, root);
  return path && opensInFiles(path) ? path : null;
}

/** Find unquoted paths in prose, leaving adjacent text and line numbers outside. */
export function findMessagePathRanges(text: string, root: string): MessagePathRange[] {
  if (!root) return [];
  const windows = isWindowsRoot(root);
  // A relative path starts with an ASCII directory so text before it does not become
  // part of the name. Absolute paths can start with any letter: the root check below
  // decides whether they belong to this workspace.
  const token = windows
    ? /(?:[A-Za-z]:[\\/]|\\\\|\/)[\p{L}\p{N}\p{M}_.@+-]+(?:[\\/][\p{L}\p{N}\p{M}_.@+-]+)*|[A-Za-z0-9_.@+-]+(?:[\\/][\p{L}\p{N}\p{M}_.@+-]+)*/gu
    : /\/[\p{L}\p{N}\p{M}_.@+-]+(?:\/[\p{L}\p{N}\p{M}_.@+-]+)*|[A-Za-z0-9_.@+-]+(?:\/[\p{L}\p{N}\p{M}_.@+-]+)*/gu;
  const links = findMessageLinkRanges(text);
  const ranges: MessagePathRange[] = [];
  let linkIndex = 0;
  let unfinishedAbsoluteEnd: number | null = null;
  for (let match = token.exec(text); match; match = token.exec(text)) {
    const start = match.index;
    let candidate = match[0];
    // Stop at a known extension followed directly by non-ASCII prose, then scan
    // the rest as well: "docs/a.md和docs/b.md" contains two paths.
    for (const extension of candidate.matchAll(
      /\.[A-Za-z0-9]+(?=[\p{L}\p{N}\p{M}])(?![A-Za-z0-9])/gu,
    )) {
      const end = extension.index + extension[0].length;
      if (opensInFiles(candidate.slice(0, end))) {
        candidate = candidate.slice(0, end);
        token.lastIndex = start + end;
        break;
      }
    }
    const tokenEnd = start + candidate.length;
    // A space can split an absolute path in prose. Do not mistake a later fragment for a
    // relative file; inline code can express the whole path without this ambiguity.
    if (
      unfinishedAbsoluteEnd !== null &&
      /^[ \t]+$/u.test(text.slice(unfinishedAbsoluteEnd, start))
    ) {
      unfinishedAbsoluteEnd = tokenEnd;
      continue;
    }
    unfinishedAbsoluteEnd = null;
    const absolute = windows ? /^(?:[A-Za-z]:[\\/]|[\\/])/u : /^\//u;
    if (start > 0 && text[start - 1] === "~" && absolute.test(candidate)) {
      if (!opensInFiles(candidate)) unfinishedAbsoluteEnd = tokenEnd;
      continue;
    }
    // Do not turn the suffix of a home path, URL, or another path into a separate path.
    if (start > 0 && /[A-Za-z0-9_.@+~/\\-]/u.test(text[start - 1])) continue;
    // An ASCII colon may introduce a path in prose, but not a drive or URL prefix.
    if (start > 0 && text[start - 1] === ":" && /^[\\/]/u.test(candidate)) continue;
    const value = candidate.replace(/\.+$/u, "");
    const end = start + value.length;
    while (linkIndex < links.length && links[linkIndex].end <= start) linkIndex += 1;
    if (linkIndex < links.length && end > links[linkIndex].start) continue;
    if (!(windows ? /[\\/]/u : /\//u).test(value)) continue;
    if (absolute.test(value) && !opensInFiles(value)) {
      unfinishedAbsoluteEnd = tokenEnd;
      continue;
    }
    const path = workspaceRelativePath(value, root);
    if (path && opensInFiles(path)) ranges.push({ start, end, path });
  }
  return ranges;
}

/** Inline code may quote one path with spaces or non-ASCII file names. */
export function findMessageInlinePathRanges(text: string, root: string): MessagePathRange[] {
  if (!root) return [];
  const ordinary = findMessagePathRanges(text, root);
  if (ordinary.length > 1) return ordinary;
  const start = text.search(/\S/u);
  if (start < 0) return ordinary;
  const value = text.trim().replace(/(?::\d+){1,2}$/u, "");
  const windows = isWindowsRoot(root);
  const separator = value.search(windows ? /[\\/]/u : /\//u);
  const firstSpace = value.search(/\s/u);
  if (separator < 0 || (firstSpace >= 0 && firstSpace < separator) || /[\r\n]/u.test(value))
    return ordinary;
  const afterDrive = windows && /^[A-Za-z]:[\\/]/u.test(value) ? value.slice(2) : value;
  if (afterDrive.includes(":")) return ordinary;
  const path = workspaceRelativePath(value, root);
  if (path && opensInFiles(path)) return [{ start, end: start + value.length, path }];
  // An absolute path outside the workspace must not turn its suffix into a relative path.
  if (/^(?:[A-Za-z]:[\\/]|[\\/])/u.test(value)) return [];
  return ordinary;
}
