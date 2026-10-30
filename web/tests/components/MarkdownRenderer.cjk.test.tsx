// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

import { MarkdownRenderer } from "../../src/components/MarkdownRenderer";
import { MarkdownDocumentSurface } from "../../src/components/document/MarkdownDocumentSurface";
import { MessageContent } from "../../src/components/messageBubble/MessageContent";

function renderMarkdown(content: string): HTMLDivElement {
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(<MarkdownRenderer content={content} />);
  return container;
}

describe("CJK Markdown emphasis", () => {
  it.each([
    ["**桌面端：**检查通过", "strong", "桌面端："],
    ["**完了：**確認してください", "strong", "完了："],
    ["前文**（重要）**继续", "strong", "（重要）"],
    ["*提示：*下一步", "em", "提示："],
    ["~~过期：~~当前", "s", "过期："],
  ])("recognizes adjacent CJK punctuation in %s", (source, tag, text) => {
    const content = renderMarkdown(source);
    expect(content.querySelector(tag)?.textContent).toBe(text);
  });

  it("keeps established English, mathematical, ordered-list and spaced emphasis behavior", () => {
    const content = renderMarkdown(
      "a*b*c\nsnake_case_name\n2*3*4\n\n1. 第一\n2. 第二\n\n**粗体：** 文字",
    );
    const paragraphs = content.querySelectorAll("p");
    expect(paragraphs[0].innerHTML).toContain("a<em>b</em>c");
    expect(paragraphs[0].textContent).toContain("snake_case_name");
    expect(paragraphs[0].innerHTML).toContain("2<em>3</em>4");
    expect([...content.querySelectorAll("ol > li")].map((item) => item.textContent)).toEqual([
      "第一",
      "第二",
    ]);
    expect(paragraphs[1].innerHTML).toBe("<strong>粗体：</strong> 文字");
  });

  it("preserves escaped markers and code, keeps raw HTML and unsafe links inert", () => {
    const content = renderMarkdown(
      [
        "\\*\\*桌面端：\\*\\*检查",
        "`**桌面端：**检查`",
        "```text\n**桌面端：**检查\n```",
        "    **桌面端：**检查",
        "<img src=x onerror=alert(1)>",
        "[**注意：**](javascript:alert(1))查看",
      ].join("\n\n"),
    );
    const paragraphs = content.querySelectorAll("p");
    expect(paragraphs[0].textContent).toBe("**桌面端：**检查");
    expect(paragraphs[0].querySelector("strong")).toBeNull();
    expect(paragraphs[1].querySelector("code")?.textContent).toBe("**桌面端：**检查");
    expect([...content.querySelectorAll("pre code")].map((code) => code.textContent)).toEqual([
      "**桌面端：**检查\n",
      "**桌面端：**检查\n",
    ]);
    expect(content.querySelector("img")).toBeNull();
    expect(content.textContent).toContain("<img src=x onerror=alert(1)>");
    expect(content.querySelector('a[href^="javascript:"]')).toBeNull();
  });

  it("retains the current underscore limitation and the existing single-star streaming frame", () => {
    expect(renderMarkdown("__提醒：__下一步").textContent).toBe("__提醒：__下一步\n");
    expect(renderMarkdown("**桌面端：").querySelector("p")?.innerHTML).toBe("**桌面端：");
    expect(renderMarkdown("**桌面端：*").querySelector("p")?.innerHTML).toBe("*<em>桌面端：</em>");
    expect(renderMarkdown("**桌面端：**").querySelector("strong")?.textContent).toBe("桌面端：");
    const continued = renderMarkdown("**桌面端：**检查");
    expect(continued.querySelector("strong")?.textContent).toBe("桌面端：");
    expect(continued.querySelector("p")?.textContent).toBe("桌面端：检查");
  });

  it("updates a streaming message as its emphasis closes and more text arrives", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      for (const [prefix, expected] of [
        ["**桌面端：", "**桌面端："],
        ["**桌面端：*", "*<em>桌面端：</em>"],
        ["**桌面端：**", "<strong>桌面端：</strong>"],
        ["**桌面端：**检查", "<strong>桌面端：</strong>检查"],
      ]) {
        await act(async () => root.render(<MarkdownRenderer content={prefix} />));
        expect(host.querySelector("p")?.innerHTML).toBe(expected);
      }
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("keeps document-relative URL resolution with CJK emphasis", () => {
    const content = document.createElement("div");
    content.innerHTML = renderToStaticMarkup(
      <MarkdownRenderer
        content="[说明](guide.md) **参考：**请阅读"
        resolveUrl={(url) => `/preview/${url}`}
      />,
    );
    expect(content.querySelector("a")?.getAttribute("href")).toBe("/preview/guide.md");
    expect(content.querySelector("strong")?.textContent).toBe("参考：");
  });

  it("renders the same CJK emphasis in message and document surfaces", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <>
            <div data-surface="message">
              <MessageContent fallbackText="**状态：**已完成" shouldRenderMarkdown isDark={false} />
            </div>
            <div data-surface="document">
              <MarkdownDocumentSurface content="**状态：**已完成" />
            </div>
          </>,
        );
      });
      await vi.waitFor(() => {
        expect(host.querySelector('[data-surface="message"] strong')?.textContent).toBe("状态：");
        expect(host.querySelector('[data-surface="document"] strong')?.textContent).toBe("状态：");
      });
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
