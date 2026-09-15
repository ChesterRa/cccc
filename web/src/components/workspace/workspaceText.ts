/** The browser textarea exposes LF even when the file contains CRLF or CR. */
export function editorText(content: string): string {
  return content.replace(/\r\n?/g, "\n");
}

/** Apply the changed textarea range while keeping untouched file bytes, including mixed EOLs. */
export function applyEditorText(previous: string, next: string, baseline: string): string {
  const before = editorText(previous);
  if (before === next) return previous;

  let start = 0;
  while (start < before.length && start < next.length && before[start] === next[start]) start++;
  let end = before.length;
  let nextEnd = next.length;
  while (end > start && nextEnd > start && before[end - 1] === next[nextEnd - 1]) {
    end--;
    nextEnd--;
  }

  // Translate normalized textarea offsets back to offsets in the retained file text.
  const fileOffset = (offset: number) => {
    let raw = 0;
    for (let normalized = 0; normalized < offset; normalized++, raw++) {
      if (previous[raw] === "\r" && previous[raw + 1] === "\n") raw++;
    }
    return raw;
  };
  const newline = baseline.match(/\r\n|\r|\n/)?.[0] ?? "\n";
  return (
    previous.slice(0, fileOffset(start)) +
    next.slice(start, nextEnd).replace(/\n/g, newline) +
    previous.slice(fileOffset(end))
  );
}
