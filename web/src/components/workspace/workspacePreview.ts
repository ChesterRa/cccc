import type { WorkspaceFile } from "../../types";

export function workspacePreviewKind(file: WorkspaceFile) {
  const mime = file.mime_type.toLowerCase().split(";")[0];
  const extension = file.path.split(".").pop()?.toLowerCase();
  if (mime.startsWith("image/")) return "image";
  // MIME is an extension hint: TypeScript shares .ts/.mts with transport streams,
  // and audio playlists are text. Use the read result before choosing a player.
  if (mime.startsWith("video/")) return file.binary ? "video" : "text";
  if (mime.startsWith("audio/")) return file.binary ? "audio" : "text";
  if (mime === "application/pdf") return "pdf";
  if (file.binary || file.truncated) return "text";
  if (mime === "text/markdown" || extension === "md" || extension === "markdown") return "markdown";
  if (mime === "text/csv" || extension === "csv") return "csv";
  if (mime === "text/tab-separated-values" || extension === "tsv") return "tsv";
  if (mime === "text/html" || extension === "html" || extension === "htm") return "html";
  return "text";
}

/** Resolve document URLs without converting literal POSIX backslashes to separators. */
export function workspaceLink(
  filePath: string,
  href: string,
): { path: string; hash: string } | null {
  if (!href || /^[a-z][a-z\d+.-]*:/i.test(href) || href.startsWith("//")) return null;
  const [rawPath] = href.split(/[?#]/);
  const hashIndex = href.indexOf("#");
  const hash = hashIndex >= 0 ? href.slice(hashIndex) : "";
  if (!rawPath) return { path: filePath, hash };
  let decoded: string;
  try {
    decoded = decodeURIComponent(rawPath);
  } catch {
    return null;
  }
  if (decoded.includes("\0")) return null;
  const parts = decoded.startsWith("/") ? [] : filePath.split("/").slice(0, -1);
  for (const part of decoded.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (!parts.length) return null;
      parts.pop();
    } else parts.push(part);
  }
  return parts.length ? { path: parts.join("/"), hash } : null;
}

/** RFC-style quoted fields, with bounded DOM storage. Source text is never changed. */
export function parseWorkspaceTable(text: string, delimiter: "," | "\t") {
  const rows: string[][] = [];
  let row: string[] = [],
    field = "",
    quoted = false,
    closed = false,
    touched = false;
  let rowCount = 0,
    columns = 0,
    columnCount = 0;
  let error = false;
  const endField = () => {
    if (rowCount < 200 && columnCount < 50) row.push(field);
    field = "";
    columnCount++;
    closed = false;
  };
  const endRow = () => {
    endField();
    columns = Math.max(columns, columnCount);
    if (rowCount < 200) rows.push(row);
    rowCount++;
    row = [];
    columnCount = 0;
    touched = false;
  };
  const input = text.replace(/^\uFEFF/, "");
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (quoted) {
      if (ch === '"') {
        if (input[i + 1] === '"') {
          field += '"';
          i++;
        } else {
          quoted = false;
          closed = true;
        }
      } else field += ch;
    } else if (ch === delimiter) {
      endField();
      touched = true;
    } else if (ch === "\n" || ch === "\r") {
      endRow();
      if (ch === "\r" && input[i + 1] === "\n") i++;
    } else if (ch === '"' && !field && !closed) {
      quoted = true;
      touched = true;
    } else {
      if (closed || ch === '"') error = true;
      field += ch;
      touched = true;
    }
  }
  if (quoted) error = true;
  if (touched || field || closed || columnCount) endRow();
  return { rows, rowCount, columns, limited: rowCount > 200 || columns > 50, error };
}
