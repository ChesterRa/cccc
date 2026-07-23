// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const mermaidMock = vi.hoisted(() => ({ initialize: vi.fn(), render: vi.fn() }));

vi.mock("mermaid", () => ({ default: mermaidMock }));

import {
  activateMermaidBlocks,
  buildMermaidConfig,
  canRenderMermaidSource,
  containsMermaidImageShape,
  isMermaidFenceLanguage,
  MERMAID_MAX_SOURCE_LENGTH,
  toggleMermaidView,
} from "../../src/utils/mermaid";

function createBlock(source: string): HTMLDivElement {
  const container = document.createElement("div");
  container.innerHTML = `
    <div data-mermaid-block data-mermaid-state="pending">
      <button data-cccc-mermaid-expand hidden>Expand</button>
      <button class="mermaid-view-toggle" data-cccc-mermaid-toggle data-view-source-label="View source" data-view-diagram-label="View diagram">View source</button>
      <button class="copy-button" data-cccc-markdown-copy data-code="${encodeURIComponent(source)}">Copy</button>
      <div class="mermaid-status" data-cccc-mermaid-status>Rendering</div>
      <div class="mermaid-render-target" data-cccc-mermaid-target></div>
      <div class="mermaid-error" data-cccc-mermaid-error hidden data-render-failed="Render failed" data-too-large="Too large"></div>
      <pre class="mermaid-source" data-cccc-mermaid-source hidden><code></code></pre>
    </div>`;
  document.body.append(container);
  return container;
}

describe("Mermaid message rendering", () => {
  beforeEach(() => {
    mermaidMock.initialize.mockReset();
    mermaidMock.render.mockReset();
  });

  afterEach(() => {
    document.body.replaceChildren();
  });

  it("recognizes only an explicit Mermaid fence language", () => {
    expect(isMermaidFenceLanguage("mermaid")).toBe(true);
    expect(isMermaidFenceLanguage(" MERMAID ")).toBe(true);
    expect(isMermaidFenceLanguage("diagram")).toBe(false);
    expect(isMermaidFenceLanguage("mermaid-js")).toBe(false);
  });

  it("uses strict bounded Mermaid configuration", () => {
    expect(buildMermaidConfig("dark")).toMatchObject({
      startOnLoad: false,
      securityLevel: "strict",
      suppressErrorRendering: true,
      theme: "dark",
      maxTextSize: MERMAID_MAX_SOURCE_LENGTH,
    });
    expect(canRenderMermaidSource("x".repeat(MERMAID_MAX_SOURCE_LENGTH))).toBe(true);
    expect(canRenderMermaidSource("x".repeat(MERMAID_MAX_SOURCE_LENGTH + 1))).toBe(false);
  });

  it("recognizes flowchart image shapes that can stall Mermaid's render queue", () => {
    expect(
      containsMermaidImageShape('flowchart TD\n  A@{ img: "https://example.test/a.png" }'),
    ).toBe(true);
    expect(containsMermaidImageShape("flowchart TD\n  A@{\n    IMG : './local.png'\n  }")).toBe(
      true,
    );
    expect(containsMermaidImageShape('flowchart TD\n  A["img: ordinary label"] --> B')).toBe(false);
  });

  it("renders asynchronously and preserves a reversible source view", async () => {
    mermaidMock.render.mockResolvedValue({ svg: '<svg data-rendered="true"></svg>' });
    const container = createBlock("flowchart TD\n  A --> B");
    const stop = activateMermaidBlocks(container, {
      theme: "dark",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });

    await vi.waitFor(() => {
      expect(container.querySelector("[data-rendered=true]")).not.toBeNull();
    });
    const block = container.querySelector<HTMLElement>("[data-mermaid-block]")!;
    const toggle = container.querySelector<HTMLButtonElement>(".mermaid-view-toggle")!;
    const target = container.querySelector<HTMLElement>(".mermaid-render-target")!;
    const source = container.querySelector<HTMLElement>(".mermaid-source")!;
    const expand = container.querySelector<HTMLButtonElement>("[data-cccc-mermaid-expand]")!;
    expect(mermaidMock.initialize).toHaveBeenCalledWith(
      expect.objectContaining({ securityLevel: "strict", theme: "dark" }),
    );
    expect(block.dataset.mermaidState).toBe("rendered");
    expect(source.hidden).toBe(true);
    expect(expand.hidden).toBe(false);

    expect(toggleMermaidView(toggle)).toBe(true);
    expect(source.hidden).toBe(false);
    expect(target.hidden).toBe(true);
    expect(toggle.textContent).toBe("View diagram");
    expect(expand.hidden).toBe(true);

    expect(toggleMermaidView(toggle)).toBe(true);
    expect(source.hidden).toBe(true);
    expect(target.hidden).toBe(false);
    expect(toggle.textContent).toBe("View source");
    expect(expand.hidden).toBe(false);
    stop();
  });

  it("keeps renderer controls separate from Mermaid-defined SVG classes", async () => {
    mermaidMock.render.mockResolvedValue({
      svg: '<svg><g class="copy-button mermaid-view-toggle mermaid-render-target mermaid-source mermaid-status mermaid-error" data-svg-collision="true"></g></svg>',
    });
    const container = createBlock("flowchart TD\n  A --> B");
    const stop = activateMermaidBlocks(container, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });

    await vi.waitFor(() => {
      expect(container.querySelector("[data-svg-collision=true]")).not.toBeNull();
    });
    const toggle = container.querySelector<HTMLButtonElement>("[data-cccc-mermaid-toggle]")!;
    const source = container.querySelector<HTMLElement>("[data-cccc-mermaid-source]")!;
    const target = container.querySelector<HTMLElement>("[data-cccc-mermaid-target]")!;

    expect(toggleMermaidView(toggle)).toBe(true);
    expect(source.hidden).toBe(false);
    expect(target.hidden).toBe(true);
    expect(container.querySelector("[data-svg-collision=true]")).not.toBeNull();
    stop();
  });

  it("skips canceled jobs that have not started rendering", async () => {
    let finishFirstRender: ((result: { svg: string }) => void) | undefined;
    mermaidMock.render
      .mockImplementationOnce(
        () =>
          new Promise<{ svg: string }>((resolve) => {
            finishFirstRender = resolve;
          }),
      )
      .mockResolvedValueOnce({ svg: '<svg data-rendered="active"></svg>' });

    const first = createBlock("flowchart TD\n  first");
    const stopFirst = activateMermaidBlocks(first, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });
    await vi.waitFor(() => expect(mermaidMock.render).toHaveBeenCalledTimes(1));

    const canceled = createBlock("flowchart TD\n  canceled");
    const stopCanceled = activateMermaidBlocks(canceled, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });
    stopCanceled();

    const active = createBlock("flowchart TD\n  active");
    const stopActive = activateMermaidBlocks(active, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });

    finishFirstRender?.({ svg: '<svg data-rendered="first"></svg>' });
    await vi.waitFor(() => {
      expect(active.querySelector('[data-rendered="active"]')).not.toBeNull();
    });

    expect(mermaidMock.render).toHaveBeenCalledTimes(2);
    expect(mermaidMock.render.mock.calls.map((call) => call[1])).toEqual([
      "flowchart TD\n  first",
      "flowchart TD\n  active",
    ]);
    expect(canceled.querySelector("[data-rendered]")).toBeNull();
    stopFirst();
    stopActive();
  });

  it("falls back to visible source when parsing fails", async () => {
    mermaidMock.render.mockRejectedValue(new Error("invalid diagram"));
    const container = createBlock("not a diagram");
    activateMermaidBlocks(container, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });

    await vi.waitFor(() => {
      expect(
        container.querySelector<HTMLElement>("[data-mermaid-block]")?.dataset.mermaidState,
      ).toBe("failed");
    });
    expect(container.querySelector<HTMLElement>(".mermaid-source")?.hidden).toBe(false);
    expect(container.querySelector<HTMLElement>(".mermaid-error")?.textContent).toBe(
      "Render failed",
    );
    expect(container.querySelector<HTMLButtonElement>(".mermaid-view-toggle")?.hidden).toBe(true);
  });

  it("rejects oversized input before loading the renderer", () => {
    const container = createBlock("x".repeat(MERMAID_MAX_SOURCE_LENGTH + 1));
    activateMermaidBlocks(container, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });

    expect(mermaidMock.render).not.toHaveBeenCalled();
    expect(container.querySelector<HTMLElement>(".mermaid-source")?.hidden).toBe(false);
    expect(container.querySelector<HTMLElement>(".mermaid-error")?.textContent).toBe("Too large");
  });

  it("falls back before loading Mermaid for an image shape", async () => {
    const image = createBlock(
      'flowchart TD\n  A@{ img: "https://attacker.example/pixel.png", h: 32, w: 32 }',
    );
    activateMermaidBlocks(image, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });

    expect(mermaidMock.render).not.toHaveBeenCalled();
    expect(image.querySelector<HTMLElement>("[data-mermaid-block]")?.dataset.mermaidState).toBe(
      "failed",
    );
    expect(image.querySelector<HTMLElement>(".mermaid-source")?.hidden).toBe(false);

    mermaidMock.render.mockResolvedValue({ svg: '<svg data-rendered="next"></svg>' });
    const next = createBlock("flowchart TD\n  A --> B");
    activateMermaidBlocks(next, {
      theme: "default",
      renderFailedLabel: "Render failed",
      tooLargeLabel: "Too large",
    });
    await vi.waitFor(() => {
      expect(next.querySelector('[data-rendered="next"]')).not.toBeNull();
    });
    expect(mermaidMock.render).toHaveBeenCalledTimes(1);
  });
});
