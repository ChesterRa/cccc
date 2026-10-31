import { describe, expect, it } from "vite-plus/test";
import {
  documentLineEnding,
  documentTextareaText,
  serializeDocumentText,
} from "./documentLineEndings";

describe("Voice Secretary document line endings", () => {
  it.each([
    ["first\r\nsecond\r\n", "\r\n"],
    ["first\nsecond\n", "\n"],
    ["first\rsecond\r", "\r"],
    ["first\r\nsecond\n", "\n"],
    ["no break", "\n"],
  ] as const)("serializes edits with the source convention: %j", (source, ending) => {
    expect(documentLineEnding(source)).toBe(ending);
    const edited = `${documentTextareaText(source)}added\n`;
    expect(serializeDocumentText(edited, ending)).toBe(edited.replace(/\n/g, ending));
  });
});
