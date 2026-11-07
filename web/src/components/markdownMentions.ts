import type MarkdownIt from "markdown-it";
import type { Token } from "markdown-it";
import {
  findMessageLinkRanges,
  findMessageMentionRanges,
  type MessageMentionToken,
} from "../utils/mentionHighlight";
import { findMessagePathRanges } from "../utils/messagePaths";

/**
 * Wraps the references listed in `env.mentionTokens` in `<span data-message-mention>`, with the
 * agent's accent slot in `data-mention-accent`.
 * Only plain text is marked: link text, inline code and code blocks keep their own formatting.
 */
export function markdownMentions(md: MarkdownIt) {
  // Linkify treats CJK punctuation and the prose after it as part of the URL.
  // Only adjust automatically detected links; explicit Markdown links keep their target.
  md.core.ruler.push("message_link_boundaries", (state) => {
    for (const block of state.tokens) {
      if (block.type !== "inline" || !block.children) continue;
      const children = block.children;
      for (let index = 0; index + 2 < children.length; index += 1) {
        const open = children[index];
        const text = children[index + 1];
        const close = children[index + 2];
        if (
          open.type !== "link_open" ||
          open.markup !== "linkify" ||
          text.type !== "text" ||
          close.type !== "link_close"
        )
          continue;
        // Fuzzy domains turn common file names such as AGENTS.md into external links.
        // Keep explicit protocols, www links and email addresses.
        if (
          !/^(?:[A-Za-z][A-Za-z\d+.-]*:\/\/|www\.)/iu.test(text.content) &&
          !/^mailto:/iu.test(open.attrGet("href") || "")
        ) {
          children.splice(index, 3, text);
          continue;
        }
        const ranges = findMessageLinkRanges(text.content);
        if (
          !ranges.length ||
          ranges[0].start !== 0 ||
          (ranges.length === 1 && ranges[0].end === text.content.length)
        )
          continue;
        const parts: Token[] = [];
        const addText = (value: string) => {
          if (!value) return;
          const token = new state.Token("text", "", 0);
          token.content = value;
          parts.push(token);
        };
        let cursor = 0;
        for (const range of ranges) {
          addText(text.content.slice(cursor, range.start));
          const link = new state.Token("link_open", "a", 1);
          link.markup = "linkify";
          link.info = "auto";
          link.attrSet("href", md.normalizeLink(range.href));
          parts.push(link);
          addText(text.content.slice(range.start, range.end));
          parts.push(new state.Token("link_close", "a", -1));
          cursor = range.end;
        }
        addText(text.content.slice(cursor));
        children.splice(index, 3, ...parts);
        index += parts.length - 1;
      }
    }
  });
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
        const paths = findMessagePathRanges(child.content, state.env?.pathRoot || "");
        const ranges = findMessageMentionRanges(child.content, known).filter(
          (mention) => !paths.some((path) => mention.start < path.end && path.start < mention.end),
        );
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
