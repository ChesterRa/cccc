// @vitest-environment happy-dom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

const mermaidMock = vi.hoisted(() => ({ initialize: vi.fn(), render: vi.fn() }));

vi.mock("mermaid", () => ({ default: mermaidMock }));

const TRANSLATIONS: Record<string, string> = {
  "common:copy": "Copy",
  "common:copied": "Copied",
  "common:close": "Close",
  "chat:mermaid.viewSource": "View source",
  "chat:mermaid.viewDiagram": "View diagram",
  "chat:mermaid.diagram": "Mermaid diagram",
  "chat:mermaid.expand": "Expand",
  "chat:mermaid.expandDiagram": "Expand Mermaid diagram",
  "chat:mermaid.previewHint": "Scroll to inspect large diagrams. Press Esc to close.",
  "chat:mermaid.rendering": "Rendering diagram…",
  "chat:mermaid.renderFailed": "Render failed",
  "chat:mermaid.tooLarge": "Too large",
  "chat:table.scrollRegion": "Scrollable table",
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => TRANSLATIONS[key] || key }),
}));

import { MarkdownRenderer } from "../../src/components/MarkdownRenderer";
import enChat from "../../src/i18n/locales/en/chat.json";
import jaChat from "../../src/i18n/locales/ja/chat.json";
import zhChat from "../../src/i18n/locales/zh/chat.json";

describe("MarkdownRenderer Mermaid contract", () => {
  it("keeps Mermaid rendering opt-in", () => {
    const content = "```mermaid\nflowchart TD\n  A --> B\n```";
    const ordinary = renderToStaticMarkup(<MarkdownRenderer content={content} />);
    const enabled = renderToStaticMarkup(<MarkdownRenderer content={content} enableMermaid />);

    expect(ordinary).not.toContain("data-mermaid-block");
    expect(ordinary).toContain('class="language-mermaid"');
    expect(enabled).toContain("data-mermaid-block");
    expect(enabled).toContain("data-cccc-mermaid-toggle");
    expect(enabled).toContain("data-cccc-mermaid-expand");
    expect(enabled).toContain("data-cccc-markdown-copy");
    expect(enabled).toContain("data-cccc-mermaid-target");
    expect(enabled).toContain("data-cccc-mermaid-source");
    expect(enabled).toContain("Rendering diagram…");
    expect(enabled).toContain("View source");
  });

  it("keeps untrusted diagram source encoded and escaped", () => {
    const source = 'flowchart TD\n  A["<img src=x onerror=alert(1)>"] --> B';
    const html = renderToStaticMarkup(
      <MarkdownRenderer content={`\`\`\`mermaid\n${source}\n\`\`\``} enableMermaid />,
    );

    expect(html).not.toContain("<img src=x");
    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;");
    expect(html).toContain("%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E");
  });

  it("does not treat Mermaid-defined SVG classes as renderer controls", async () => {
    mermaidMock.initialize.mockReset();
    mermaidMock.render.mockReset();
    mermaidMock.render.mockResolvedValue({
      svg: '<svg viewBox="0 0 320 180"><g class="copy-button mermaid-view-toggle mermaid-source" data-svg-collision="true"></g></svg>',
    });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <MarkdownRenderer content={"```mermaid\nflowchart TD\n  A --> B\n```"} enableMermaid />,
      );
    });
    await vi.waitFor(() => {
      expect(host.querySelector("[data-svg-collision=true]")).not.toBeNull();
    });

    const collision = host.querySelector<SVGGElement>("[data-svg-collision=true]")!;
    const source = host.querySelector<HTMLElement>("[data-cccc-mermaid-source]")!;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    let dispatched = true;
    await act(async () => {
      dispatched = collision.dispatchEvent(click);
    });
    expect(dispatched).toBe(false);
    expect(click.defaultPrevented).toBe(true);
    expect(source.hidden).toBe(true);
    await vi.waitFor(() => {
      expect(
        document.body.querySelector('[role="dialog"] [data-svg-collision=true]'),
      ).not.toBeNull();
    });

    await act(async () => root.unmount());
    host.remove();
  });

  it("preserves a rendered diagram across unrelated parent renders", async () => {
    mermaidMock.initialize.mockReset();
    mermaidMock.render.mockReset();
    mermaidMock.render.mockResolvedValue({ svg: '<svg data-stable-diagram="true"></svg>' });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const content = "```mermaid\nflowchart TD\n  A --> B\n```";

    await act(async () => {
      root.render(<MarkdownRenderer content={content} enableMermaid className="first-render" />);
    });
    await vi.waitFor(() => {
      expect(host.querySelector("[data-stable-diagram=true]")).not.toBeNull();
    });
    const renderedDiagram = host.querySelector("[data-stable-diagram=true]");

    await act(async () => {
      root.render(<MarkdownRenderer content={content} enableMermaid className="second-render" />);
    });

    expect(host.querySelector("[data-stable-diagram=true]")).toBe(renderedDiagram);
    expect(mermaidMock.render).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    host.remove();
  });

  it("opens the completed SVG in a keyboard-safe preview without rendering it again", async () => {
    mermaidMock.initialize.mockReset();
    mermaidMock.render.mockReset();
    mermaidMock.render.mockResolvedValue({
      svg: '<svg viewBox="0 0 640 320" data-preview-diagram="true"></svg>',
    });
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <MarkdownRenderer content={"```mermaid\nflowchart TD\n  A --> B\n```"} enableMermaid />,
      );
    });
    await vi.waitFor(() => {
      expect(host.querySelector("[data-preview-diagram=true]")).not.toBeNull();
    });

    const expand = host.querySelector<HTMLButtonElement>("[data-cccc-mermaid-expand]")!;
    expect(expand.hidden).toBe(false);
    const target = host.querySelector<HTMLElement>("[data-cccc-mermaid-target]")!;
    await act(async () => target.click());

    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog).not.toBeNull();
    expect(dialog.querySelector("[data-preview-diagram=true]")).not.toBeNull();
    expect(dialog.textContent).toContain("Scroll to inspect large diagrams");
    expect(mermaidMock.render).toHaveBeenCalledTimes(1);

    const sourceToggle = Array.from(dialog.querySelectorAll("button")).find(
      (button) => button.textContent === "View source",
    )!;
    await act(async () => sourceToggle.click());
    expect(dialog.textContent).toContain("flowchart TD");

    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();

    await act(async () => root.unmount());
    host.remove();
  });

  it("enables diagrams only on Chat and Inbox message surfaces", () => {
    const messageBubble = readFileSync(
      resolve(process.cwd(), "src/components/MessageBubble.tsx"),
      "utf8",
    );
    const messageContent = readFileSync(
      resolve(process.cwd(), "src/components/messageBubble/MessageContent.tsx"),
      "utf8",
    );
    const inbox = readFileSync(
      resolve(process.cwd(), "src/components/modals/InboxModal.tsx"),
      "utf8",
    );
    const documentSurface = readFileSync(
      resolve(process.cwd(), "src/components/document/MarkdownDocumentSurface.tsx"),
      "utf8",
    );

    expect(messageBubble).toContain('from "./messageBubble/MessageContent"');
    expect(messageContent).toContain("enableMermaid");
    expect(inbox).toContain("enableMermaid");
    expect(documentSurface).not.toContain("enableMermaid");
  });

  it("ships the complete message diagram labels in every supported locale", () => {
    for (const chat of [enChat, jaChat, zhChat]) {
      expect(chat.mermaid.diagram).toBeTruthy();
      expect(chat.mermaid.expand).toBeTruthy();
      expect(chat.mermaid.expandDiagram).toBeTruthy();
      expect(chat.mermaid.previewHint).toBeTruthy();
      expect(chat.mermaid.viewSource).toBeTruthy();
      expect(chat.mermaid.viewDiagram).toBeTruthy();
      expect(chat.mermaid.rendering).toBeTruthy();
      expect(chat.mermaid.renderFailed).toBeTruthy();
      expect(chat.mermaid.tooLarge).toBeTruthy();
    }
  });
});

describe("MarkdownRenderer table contract", () => {
  it("wraps Markdown tables in one keyboard-accessible horizontal scroll region", () => {
    const html = renderToStaticMarkup(
      <MarkdownRenderer
        content={
          "| Layer | Problem | Owner | Plan | Guardrail | Acceptance |\n| --- | --- | --- | --- | --- | --- |\n| Evidence | Long explanation | Qwen | Repair | No shortcuts | Verified |"
        }
      />,
    );

    expect(html).toContain(
      '<div class="markdown-table-scroll" role="region" tabindex="0" aria-label="Scrollable table"><table>',
    );
    expect(html).toMatch(/<\/table>\s*<\/div>/);
    expect(html.match(/markdown-table-scroll/g)).toHaveLength(1);
  });

  it("does not add a scroll region to ordinary Markdown", () => {
    const html = renderToStaticMarkup(<MarkdownRenderer content="A short paragraph." />);

    expect(html).not.toContain("markdown-table-scroll");
    expect(html).not.toContain('role="region"');
  });

  it("ships the table region label in every supported locale", () => {
    for (const chat of [enChat, jaChat, zhChat]) {
      expect(chat.table.scrollRegion).toBeTruthy();
    }
  });

  it("keeps overflow and column sizing scoped to the table wrapper", () => {
    const stylesheet = readFileSync(resolve(process.cwd(), "src/styles/markdown.css"), "utf8");

    expect(stylesheet).toContain(".markdown-table-scroll {");
    expect(stylesheet).toMatch(/\.markdown-table-scroll\s*\{[^}]*overflow-x:\s*auto;/s);
    expect(stylesheet).toMatch(
      /\.prose\.markdown-body \.markdown-table-scroll > table\s*\{[^}]*width:\s*max-content;[^}]*min-width:\s*100%;/s,
    );
    expect(stylesheet).toMatch(
      /\.prose\.markdown-body \.markdown-table-scroll (?:th|th,)\s*[\s\S]*?min-width:\s*12rem;/,
    );
  });
});
