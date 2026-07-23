import type { MermaidConfig } from "mermaid";

export const MERMAID_MAX_SOURCE_LENGTH = 20_000;
export const MERMAID_MAX_EDGES = 500;

let renderSequence = 0;
let renderQueue: Promise<void> = Promise.resolve();

export type MermaidColorTheme = "default" | "dark";

type MermaidActivationOptions = {
  theme: MermaidColorTheme;
  renderFailedLabel: string;
  tooLargeLabel: string;
};

export function isMermaidFenceLanguage(language: string): boolean {
  return language.trim().toLowerCase() === "mermaid";
}

export function canRenderMermaidSource(source: string): boolean {
  return source.length <= MERMAID_MAX_SOURCE_LENGTH;
}

export function containsMermaidImageShape(source: string): boolean {
  return /@\{[^{}]*\bimg\s*:/is.test(source);
}

export function buildMermaidConfig(theme: MermaidColorTheme): MermaidConfig {
  return {
    startOnLoad: false,
    securityLevel: "strict",
    suppressErrorRendering: true,
    theme,
    maxTextSize: MERMAID_MAX_SOURCE_LENGTH,
    maxEdges: MERMAID_MAX_EDGES,
  };
}

async function renderMermaidNow(source: string, theme: MermaidColorTheme): Promise<string> {
  const { default: mermaid } = await import("mermaid");
  mermaid.initialize(buildMermaidConfig(theme));
  renderSequence += 1;
  const { svg } = await mermaid.render(`cccc-mermaid-${renderSequence}`, source);
  return svg;
}

/** Mermaid configuration is process-global, so renders are serialized to prevent theme races. */
function renderMermaidDiagram(
  source: string,
  theme: MermaidColorTheme,
  isActive: () => boolean,
): Promise<string | null> {
  const render = renderQueue.then(() => (isActive() ? renderMermaidNow(source, theme) : null));
  renderQueue = render.then(
    () => undefined,
    () => undefined,
  );
  return render;
}

function showSourceFallback(block: HTMLElement, message: string): void {
  const target = block.querySelector<HTMLElement>("[data-cccc-mermaid-target]");
  const source = block.querySelector<HTMLElement>("[data-cccc-mermaid-source]");
  const status = block.querySelector<HTMLElement>("[data-cccc-mermaid-status]");
  const error = block.querySelector<HTMLElement>("[data-cccc-mermaid-error]");
  const toggle = block.querySelector<HTMLButtonElement>("button[data-cccc-mermaid-toggle]");
  const expand = block.querySelector<HTMLButtonElement>("button[data-cccc-mermaid-expand]");
  if (target) target.hidden = true;
  if (source) source.hidden = false;
  if (status) status.hidden = true;
  if (error) {
    error.textContent = message;
    error.hidden = false;
  }
  if (toggle) toggle.hidden = true;
  if (expand) expand.hidden = true;
  block.dataset.mermaidState = "failed";
}

export function activateMermaidBlocks(
  container: HTMLElement,
  options: MermaidActivationOptions,
): () => void {
  let cancelled = false;

  for (const block of container.querySelectorAll<HTMLElement>("[data-mermaid-block]")) {
    const error = block.querySelector<HTMLElement>("[data-cccc-mermaid-error]");
    const copyButton = block.querySelector<HTMLButtonElement>("button[data-cccc-markdown-copy]");
    let source = "";
    try {
      source = decodeURIComponent(copyButton?.dataset.code || "");
    } catch {
      showSourceFallback(block, error?.dataset.renderFailed || options.renderFailedLabel);
      continue;
    }
    if (!canRenderMermaidSource(source)) {
      showSourceFallback(block, error?.dataset.tooLarge || options.tooLargeLabel);
      continue;
    }
    // Mermaid 11.16 awaits Image.decode() while rendering flowchart image shapes. A failed or
    // stalled image can therefore block Mermaid's internal queue and every later message diagram.
    // Keep this unsupported niche shape as source instead of allowing one message to poison the queue.
    if (containsMermaidImageShape(source)) {
      showSourceFallback(block, error?.dataset.renderFailed || options.renderFailedLabel);
      continue;
    }

    void renderMermaidDiagram(source, options.theme, () => !cancelled && block.isConnected)
      .then((svg) => {
        if (!svg || cancelled || !block.isConnected) return;
        const target = block.querySelector<HTMLElement>("[data-cccc-mermaid-target]");
        const status = block.querySelector<HTMLElement>("[data-cccc-mermaid-status]");
        const expand = block.querySelector<HTMLButtonElement>("button[data-cccc-mermaid-expand]");
        if (!target) return;
        target.innerHTML = svg;
        if (status) status.hidden = true;
        block.dataset.mermaidState = "rendered";
        if (block.dataset.mermaidView !== "source") {
          target.hidden = false;
          if (expand) expand.hidden = false;
        }
      })
      .catch(() => {
        if (cancelled || !block.isConnected) return;
        showSourceFallback(block, error?.dataset.renderFailed || options.renderFailedLabel);
      });
  }

  return () => {
    cancelled = true;
  };
}

export function toggleMermaidView(toggle: HTMLButtonElement): boolean {
  const block = toggle.closest<HTMLElement>("[data-mermaid-block]");
  const target = block?.querySelector<HTMLElement>("[data-cccc-mermaid-target]");
  const source = block?.querySelector<HTMLElement>("[data-cccc-mermaid-source]");
  const status = block?.querySelector<HTMLElement>("[data-cccc-mermaid-status]");
  const expand = block?.querySelector<HTMLButtonElement>("button[data-cccc-mermaid-expand]");
  if (!block || !target || !source) return false;

  const showSource = block.dataset.mermaidView !== "source";
  block.dataset.mermaidView = showSource ? "source" : "diagram";
  source.hidden = !showSource;
  target.hidden = showSource;
  if (expand) expand.hidden = showSource;
  if (status) status.hidden = showSource || block.dataset.mermaidState === "rendered";
  toggle.setAttribute("aria-pressed", String(showSource));
  toggle.textContent = showSource
    ? toggle.dataset.viewDiagramLabel || ""
    : toggle.dataset.viewSourceLabel || "";
  return true;
}
