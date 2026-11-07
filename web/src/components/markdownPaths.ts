import type MarkdownIt from "markdown-it";
import type { Token } from "markdown-it";
import { findMessageInlinePathRanges, findMessagePathRanges } from "../utils/messagePaths";

/** Mark message paths without changing link text, fenced code, or other Markdown surfaces. */
export function markdownPaths(md: MarkdownIt) {
  md.core.ruler.push("message_paths", (state) => {
    const root = state.env?.pathRoot;
    if (typeof root !== "string" || !root) return;

    for (const block of state.tokens) {
      if (block.type !== "inline" || !block.children) continue;
      let linkDepth = 0;
      block.children = block.children.flatMap((child): Token[] => {
        if (child.type === "link_open") linkDepth += 1;
        if (child.type === "link_close") linkDepth -= 1;
        if (linkDepth > 0 || (child.type !== "text" && child.type !== "code_inline"))
          return [child];
        const ranges =
          child.type === "code_inline"
            ? findMessageInlinePathRanges(child.content, root)
            : findMessagePathRanges(child.content, root);
        if (!ranges.length) return [child];
        const result: Token[] = [];
        const addText = (content: string) => {
          if (!content) return;
          const token = new state.Token("text", "", 0);
          token.content = content;
          result.push(token);
        };
        if (child.type === "code_inline")
          result.push(new state.Token("message_code_open", "code", 1));
        let cursor = 0;
        for (const range of ranges) {
          addText(child.content.slice(cursor, range.start));
          const open = new state.Token("message_path_open", "span", 1);
          open.attrSet("role", "button");
          open.attrSet("tabindex", "0");
          open.attrSet("data-message-path", range.path);
          open.attrSet(
            "class",
            "cursor-pointer break-all underline underline-offset-2 hover:opacity-80",
          );
          result.push(open);
          addText(child.content.slice(range.start, range.end));
          result.push(new state.Token("message_path_close", "span", -1));
          cursor = range.end;
        }
        addText(child.content.slice(cursor));
        if (child.type === "code_inline")
          result.push(new state.Token("message_code_close", "code", -1));
        return result;
      });
    }
  });
}
