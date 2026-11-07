import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vite-plus/test";
import { MessageContent } from "./MessageContent";
import { MessageMentionText } from "./MessageMentionText";

it("marks known references in a plain message", () => {
  const markup = renderToStaticMarkup(
    <MessageContent
      fallbackText={"@claude-1 see #34 and a@b.com"}
      shouldRenderMarkdown={false}
      isDark={false}
      mentionTokens={[{ text: "@claude-1", accent: 5 }]}
    />,
  );
  expect(markup).toContain(
    '<span data-message-mention="agent" data-mention-accent="5">@claude-1</span> see #34 and a@b.com',
  );
});

it("marks a workspace path only when a root is supplied", () => {
  const text = "Open src/main.rs:12";
  const withRoot = renderToStaticMarkup(
    <MessageMentionText text={text} mentionTokens={[]} pathRoot="/repo" />,
  );
  expect(withRoot).toContain('data-message-path="src/main.rs"');
  expect(withRoot).toContain("src/main.rs</span>:12");
  const withoutRoot = renderToStaticMarkup(<MessageMentionText text={text} mentionTokens={[]} />);
  expect(withoutRoot).toBe(text);
});

it("leaves adjacent CJK prose intact and gives an overlapping path priority", () => {
  const text = "路径：docs/notes.md的第三节，@claude-1.config/a.md @claude-1";
  const markup = renderToStaticMarkup(
    <MessageMentionText
      text={text}
      mentionTokens={[{ text: "@claude-1", accent: 5 }]}
      pathRoot="/repo"
    />,
  );
  expect(markup).toContain('data-message-path="docs/notes.md"');
  expect(markup).toContain("docs/notes.md</span>的第三节");
  expect(markup).toContain('data-message-path="@claude-1.config/a.md"');
  expect(markup).toContain("@claude-1.config/a.md</span>");
  expect(markup.match(/data-message-mention=/g)).toHaveLength(1);
});

it("links bare web addresses without consuming punctuation or highlighting inside them", () => {
  const markup = renderToStaticMarkup(
    <MessageContent
      fallbackText={
        "请看https://example.test/@claude-1/report.pdf，@claude-1 再看 www.example.com。"
      }
      shouldRenderMarkdown={false}
      isDark={false}
      mentionTokens={[{ text: "@claude-1", accent: 5 }]}
    />,
  );
  expect(markup).toContain('href="https://example.test/@claude-1/report.pdf"');
  expect(markup).toContain('target="_blank" rel="noopener noreferrer"');
  expect(markup).toContain('href="http://www.example.com"');
  expect(markup).toContain(
    '</a>，<span data-message-mention="agent" data-mention-accent="5">@claude-1</span>',
  );
  expect(markup).toContain("</a>。");
});

it("keeps fullwidth parentheses and colons outside plain-message links", () => {
  const text = "请看（https://example.test/a.pdf）：https://example.test/b.md：说明";
  const markup = renderToStaticMarkup(<MessageMentionText text={text} mentionTokens={[]} />);
  expect(markup).toContain('（<a href="https://example.test/a.pdf"');
  expect(markup).toContain("https://example.test/a.pdf</a>）：");
  expect(markup).toContain('href="https://example.test/b.md"');
  expect(markup).toContain("https://example.test/b.md</a>：说明");
});
