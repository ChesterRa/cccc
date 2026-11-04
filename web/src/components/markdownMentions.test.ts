import MarkdownIt from "markdown-it";
import { expect, it } from "vite-plus/test";
import { markdownMentions } from "./markdownMentions";

const md = new MarkdownIt({ html: false, linkify: true, typographer: true, breaks: true }).use(
  markdownMentions,
);
const mentionTokens = [{ text: "@claude-1", accent: 3 }, { text: "#Target Repo" }];

it("marks the known references in text", () => {
  expect(md.render("Ask **@claude-1** about #Target Repo.", { mentionTokens })).toBe(
    '<p>Ask <strong><span data-message-mention="agent" data-mention-accent="3">@claude-1</span>' +
      "</strong> about " +
      '<span data-message-mention="group">#Target Repo</span>.</p>\n',
  );
});

it("leaves code, link text and addresses as they are", () => {
  const html = md.render(
    "`@claude-1` [@claude-1](https://example.com) claude-1@example.com\n\n```\n@claude-1\n```",
    { mentionTokens },
  );
  expect(html).not.toContain("data-message-mention");
});

it("renders unchanged without references to mark", () => {
  expect(md.render("Ask @claude-1")).toBe("<p>Ask @claude-1</p>\n");
});
