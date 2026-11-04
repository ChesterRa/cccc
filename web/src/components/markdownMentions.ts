import type MarkdownIt from "markdown-it";
import type { Token } from "markdown-it";
import { findMessageMentionRanges, type MessageMentionToken } from "../utils/mentionHighlight";

/**
 * Wraps the references listed in `env.mentionTokens` in `<span data-message-mention>`, with the
 * agent's accent slot in `data-mention-accent`.
 * Only plain text is marked: link text, inline code and code blocks keep their own formatting.
 */
export function markdownMentions(md: MarkdownIt) {
  md.core.ruler.push("message_mentions", (state) => {
    const known: readonly MessageMentionToken[] | undefined = state.env?.mentionTokens;
    if (!known?.length) return;
    const textToken = (content: string) => {
      const token = new state.Token("text", "", 0);
      token.content = content;
      return token;
    };
    for (const block of state.tokens) {
      if (block.type !== "inline" || !block.children) continue;
      let linkDepth = 0;
      block.children = block.children.flatMap((child): Token[] => {
        if (child.type === "link_open") linkDepth += 1;
        if (child.type === "link_close") linkDepth -= 1;
        if (child.type !== "text" || linkDepth > 0) return [child];
        const ranges = findMessageMentionRanges(child.content, known);
        if (!ranges.length) return [child];
        const tokens: Token[] = [];
        let cursor = 0;
        for (const range of ranges) {
          if (range.start > cursor)
            tokens.push(textToken(child.content.slice(cursor, range.start)));
          const open = new state.Token("message_mention_open", "span", 1);
          open.attrSet("data-message-mention", range.kind);
          if (range.accent !== undefined) open.attrSet("data-mention-accent", String(range.accent));
          tokens.push(
            open,
            textToken(child.content.slice(range.start, range.end)),
            new state.Token("message_mention_close", "span", -1),
          );
          cursor = range.end;
        }
        if (cursor < child.content.length) tokens.push(textToken(child.content.slice(cursor)));
        return tokens;
      });
    }
  });
}
