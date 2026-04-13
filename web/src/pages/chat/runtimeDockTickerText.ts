const TICKER_SENTENCE_BREAKS = new Set(["。", "！", "？", "；", ".", "!", "?", ";"]);
const TICKER_TRAILING_CLOSERS = new Set(["\"", "'", "”", "’", "）", ")", "]", "】", "」", "』"]);

export function splitRuntimeDockTickerText(text: string): string[] {
  const normalized = String(text || "").replace(/\r\n/g, "\n");
  const lines: string[] = [];
  let buffer = "";

  for (let index = 0; index < normalized.length; index += 1) {
    const char = normalized[index] || "";

    if (char === "\n") {
      const line = buffer.trim();
      if (line) lines.push(line);
      buffer = "";
      continue;
    }

    buffer += char;
    if (!TICKER_SENTENCE_BREAKS.has(char)) continue;

    const previousChar = normalized[index - 1] || "";
    const nextChar = normalized[index + 1] || "";
    if (char === "." && (previousChar === "." || nextChar === ".")) {
      continue;
    }

    while (TICKER_TRAILING_CLOSERS.has(normalized[index + 1] || "")) {
      index += 1;
      buffer += normalized[index] || "";
    }

    const line = buffer.trim();
    if (line) lines.push(line);
    buffer = "";
  }

  const tail = buffer.trim();
  if (tail) lines.push(tail);
  return lines;
}
