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

it("keeps Chinese punctuation and following mentions outside automatically detected links", () => {
  const html = md.render("# 标题\n\nhttps://example.test/report.pdf。@claude-1", { mentionTokens });
  expect(html).toContain(
    '<a href="https://example.test/report.pdf">https://example.test/report.pdf</a>。',
  );
  expect(html).toContain(
    '<span data-message-mention="agent" data-mention-accent="3">@claude-1</span>',
  );
});

it("preserves explicit link targets and code", () => {
  const html = md.render(
    "[说明](https://example.test/report.pdf%E3%80%82) `https://example.test/x。`",
  );
  expect(html).toContain('href="https://example.test/report.pdf%E3%80%82"');
  expect(html).toContain("<code>https://example.test/x。</code>");
});

it("keeps both bare links clickable when Chinese punctuation separates them", () => {
  expect(md.render("https://a.test/x.pdf，https://b.test/y.pdf")).toContain(
    '<a href="https://a.test/x.pdf">https://a.test/x.pdf</a>，<a href="https://b.test/y.pdf">https://b.test/y.pdf</a>',
  );
});

it("keeps fullwidth parentheses and colons outside formatted-message links", () => {
  const html = md.render("**请看**（https://example.test/a.pdf）：https://example.test/b.md：说明");
  expect(html).toContain(
    '（<a href="https://example.test/a.pdf">https://example.test/a.pdf</a>）：' +
      '<a href="https://example.test/b.md">https://example.test/b.md</a>：说明',
  );
});

it("leaves file names as text while retaining explicit links and email", () => {
  const html = md.render(
    "AGENTS.md build.rs main.py claude@example.com www.example.com https://example.com/a.md",
  );
  expect(html).toContain("AGENTS.md build.rs main.py");
  expect(html).not.toContain('href="http://AGENTS.md"');
  expect(html).not.toContain('href="http://build.rs"');
  expect(html).toContain('href="mailto:claude@example.com"');
  expect(html).toContain('href="http://www.example.com"');
  expect(html).toContain('href="https://example.com/a.md"');
});
