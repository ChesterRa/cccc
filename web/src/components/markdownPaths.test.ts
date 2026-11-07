import MarkdownIt from "markdown-it";
import { expect, it } from "vite-plus/test";
import { markdownMentions } from "./markdownMentions";
import { markdownPaths } from "./markdownPaths";

const md = new MarkdownIt({ html: false, linkify: true }).use(markdownMentions).use(markdownPaths);

it("marks in-scope paths in text and inline code but leaves the line suffix outside", () => {
  const html = md.render("See docs/guide.md:12 and `src/main.rs:4`.", { pathRoot: "/repo" });
  expect(html).toContain('data-message-path="docs/guide.md"');
  expect(html).toContain("docs/guide.md</span>:12");
  expect(html).toContain('<code><span role="button" tabindex="0" data-message-path="src/main.rs"');
  expect(html).toContain("src/main.rs</span>:4</code>");
});

it("does not mark link text, URLs, fenced code or messages without a root", () => {
  const source = "[docs/a.md](https://example.test/docs/a.md)\n\n```\ndocs/b.md\n```";
  expect(md.render(source, { pathRoot: "/repo" })).not.toContain("data-message-path");
  expect(md.render("See `docs/a.md`")).not.toContain("data-message-path");
});

it("escapes path attributes and text", () => {
  const html = md.render("`docs/a&b.md`", { pathRoot: "/repo" });
  expect(html).toContain('data-message-path="docs/a&amp;b.md"');
  expect(html).toContain("docs/a&amp;b.md</span>");
});

it("preserves spaces inside a single inline-code path", () => {
  const html = md.render("`docs/说明 一.md:3`", { pathRoot: "/repo" });
  expect(html).toContain('data-message-path="docs/说明 一.md"');
  expect(html).toContain("docs/说明 一.md</span>:3</code>");
});

it("keeps adjacent CJK prose outside a path and gives paths priority over mentions", () => {
  const html = md.render("看docs/notes.md的第三节，@claude-1.config/a.md @claude-1", {
    pathRoot: "/repo",
    mentionTokens: [{ text: "@claude-1", accent: 5 }],
  });
  expect(html).toContain('data-message-path="docs/notes.md"');
  expect(html).toContain("docs/notes.md</span>的第三节");
  expect(html).toContain('data-message-path="@claude-1.config/a.md"');
  expect(html).toContain("@claude-1.config/a.md</span>");
  expect(html.match(/data-message-mention=/g)).toHaveLength(1);
});
