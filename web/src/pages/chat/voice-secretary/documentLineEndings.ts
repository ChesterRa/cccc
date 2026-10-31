/** Textareas expose all line breaks as LF; preserve a uniform source convention on save. */
export function documentLineEnding(content: string): "\n" | "\r\n" | "\r" {
  const breaks = content.match(/\r\n|\r|\n/g) || [];
  if (breaks.length && breaks.every((value) => value === "\r\n")) return "\r\n";
  if (breaks.length && breaks.every((value) => value === "\r")) return "\r";
  // Mixed line endings are normalized to LF by the browser's textarea.
  return "\n";
}

export function documentTextareaText(content: string): string {
  return content.replace(/\r\n|\r/g, "\n");
}

export function serializeDocumentText(content: string, lineEnding: "\n" | "\r\n" | "\r"): string {
  const text = documentTextareaText(content);
  return lineEnding === "\n" ? text : text.replace(/\n/g, lineEnding);
}
