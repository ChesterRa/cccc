import { FloatingPortal } from "@floating-ui/react";
import type { CSSProperties } from "react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import MarkdownIt from "markdown-it";
import { renderToStaticMarkup } from "react-dom/server";
import { useTranslation } from "react-i18next";
import { CheckIcon, CloseIcon, CopyIcon, ExpandIcon } from "./Icons";
import { classNames } from "../utils/classNames";
import { copyTextToClipboard } from "../utils/copy";
import { activateMermaidBlocks, isMermaidFenceLanguage, toggleMermaidView } from "../utils/mermaid";
import { useModalA11y } from "../hooks/useModalA11y";

const copyIconMarkup = renderToStaticMarkup(
  <CopyIcon className="w-3.5 h-3.5" strokeWidth={2} aria-hidden="true" />,
);
const copiedIconMarkup = renderToStaticMarkup(
  <CheckIcon className="w-3.5 h-3.5" strokeWidth={2} aria-hidden="true" />,
);
const expandIconMarkup = renderToStaticMarkup(
  <ExpandIcon className="w-3.5 h-3.5" strokeWidth={2} aria-hidden="true" />,
);

type MermaidPreviewState = { svg: string; source: string; naturalWidth: number };

type MermaidLabels = {
  copy: string;
  copied: string;
  diagram: string;
  expand: string;
  expandDiagram: string;
  previewHint: string;
  viewDiagram: string;
  viewSource: string;
};

function MermaidPreviewDialog({
  preview,
  labels,
  closeLabel,
  onClose,
}: {
  preview: MermaidPreviewState;
  labels: MermaidLabels;
  closeLabel: string;
  onClose: () => void;
}) {
  const [showSource, setShowSource] = useState(false);
  const [copied, setCopied] = useState(false);
  const titleId = useId();
  const { modalRef } = useModalA11y(true, onClose);
  const renderedSvg = useMemo(() => ({ __html: preview.svg }), [preview.svg]);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const copySource = async () => {
    if (await copyTextToClipboard(preview.source)) setCopied(true);
  };

  return (
    <FloatingPortal>
      <div className="fixed inset-0 z-[90] flex items-center justify-center sm:p-4">
        <div
          className="absolute inset-0 glass-overlay"
          onPointerDown={onClose}
          aria-hidden="true"
        />
        <div
          ref={modalRef}
          className="glass-modal relative z-[91] flex h-[100dvh] w-full flex-col overflow-hidden rounded-none border shadow-2xl sm:h-[min(92vh,64rem)] sm:w-[min(96vw,100rem)] sm:rounded-2xl"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
        >
          <div className="flex flex-shrink-0 items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-3 py-2.5 sm:px-4">
            <div className="min-w-0">
              <p
                id={titleId}
                className="truncate text-sm font-semibold text-[var(--color-text-primary)]"
              >
                {labels.diagram}
              </p>
              <p className="hidden truncate text-xs text-[var(--color-text-tertiary)] sm:block">
                {labels.previewHint}
              </p>
            </div>
            <div className="flex flex-shrink-0 items-center gap-1.5">
              <button
                type="button"
                className="mermaid-preview-action"
                onClick={() => setShowSource((current) => !current)}
                aria-pressed={showSource}
              >
                {showSource ? labels.viewDiagram : labels.viewSource}
              </button>
              <button
                type="button"
                className="mermaid-preview-action"
                onClick={() => void copySource()}
              >
                {copied ? (
                  <CheckIcon className="h-3.5 w-3.5 text-green-600 dark:text-emerald-400" />
                ) : (
                  <CopyIcon className="h-3.5 w-3.5" />
                )}
                <span className="hidden sm:inline">{copied ? labels.copied : labels.copy}</span>
              </button>
              <button
                type="button"
                className="mermaid-preview-action mermaid-preview-close"
                onClick={onClose}
                aria-label={closeLabel}
                title={closeLabel}
              >
                <CloseIcon className="h-4 w-4" />
              </button>
            </div>
          </div>

          {showSource ? (
            <pre className="mermaid-preview-source">
              <code>{preview.source}</code>
            </pre>
          ) : (
            <div className="mermaid-preview-canvas">
              <div
                className="mermaid-preview-diagram"
                style={
                  {
                    "--mermaid-preview-natural-width": `${preview.naturalWidth}px`,
                  } as CSSProperties
                }
                dangerouslySetInnerHTML={renderedSvg}
              />
            </div>
          )}
        </div>
      </div>
    </FloatingPortal>
  );
}

interface MarkdownRendererProps {
  content: string;
  isDark?: boolean;
  className?: string;
  /** Force light text (for colored backgrounds like user messages) */
  invertText?: boolean;
  /** Render Mermaid fences. Kept opt-in because this renderer is shared by non-message surfaces. */
  enableMermaid?: boolean;
}

export function MarkdownRenderer({
  content,
  isDark,
  className,
  invertText,
  enableMermaid = false,
}: MarkdownRendererProps) {
  const { t } = useTranslation(["chat", "common"]);
  const containerRef = useRef<HTMLDivElement>(null);
  const [mermaidPreview, setMermaidPreview] = useState<MermaidPreviewState | null>(null);
  const labels = {
    copy: t("common:copy"),
    copied: t("common:copied"),
    close: t("common:close"),
    viewSource: t("chat:mermaid.viewSource"),
    viewDiagram: t("chat:mermaid.viewDiagram"),
    diagram: t("chat:mermaid.diagram"),
    expand: t("chat:mermaid.expand"),
    expandDiagram: t("chat:mermaid.expandDiagram"),
    previewHint: t("chat:mermaid.previewHint"),
    rendering: t("chat:mermaid.rendering"),
    renderFailed: t("chat:mermaid.renderFailed"),
    tooLarge: t("chat:mermaid.tooLarge"),
  };

  const md = useMemo(() => {
    const instance = new MarkdownIt({
      html: false, // Security: Disable raw HTML to prevent XSS
      linkify: true,
      typographer: true,
      breaks: true,
    });
    instance.renderer.rules.fence = (tokens, idx) => {
      const token = tokens[idx];
      const info = String(token.info || "").trim();
      const rawLang = info.split(/\s+/)[0] || "";
      const finalLang = rawLang.toLowerCase().trim() || "text";
      const escapedLang = instance.utils.escapeHtml(finalLang);
      const escaped = instance.utils.escapeHtml(token.content || "");
      const encodedCode = encodeURIComponent(token.content || "");
      const escapedCopy = instance.utils.escapeHtml(labels.copy);
      const escapedCopied = instance.utils.escapeHtml(labels.copied);

      const copyButton =
        '<button type="button" class="copy-button flex items-center gap-1 select-none" data-cccc-markdown-copy data-code="' +
        encodedCode +
        '" aria-label="' +
        escapedCopy +
        '">' +
        '<span class="copy-icon pointer-events-none">' +
        copyIconMarkup +
        "</span>" +
        '<span class="copy-text pointer-events-none">' +
        escapedCopy +
        "</span>" +
        '<span class="copied-icon pointer-events-none hidden text-green-500 dark:text-emerald-400">' +
        copiedIconMarkup +
        "</span>" +
        '<span class="copied-text pointer-events-none hidden text-green-500 dark:text-emerald-400">' +
        escapedCopied +
        "</span>" +
        "</button>";

      if (enableMermaid && isMermaidFenceLanguage(finalLang)) {
        const escapedViewSource = instance.utils.escapeHtml(labels.viewSource);
        const escapedViewDiagram = instance.utils.escapeHtml(labels.viewDiagram);
        const escapedExpand = instance.utils.escapeHtml(labels.expand);
        const escapedExpandDiagram = instance.utils.escapeHtml(labels.expandDiagram);
        const escapedRendering = instance.utils.escapeHtml(labels.rendering);
        const escapedRenderFailed = instance.utils.escapeHtml(labels.renderFailed);
        const escapedTooLarge = instance.utils.escapeHtml(labels.tooLarge);

        return (
          '<div class="code-block-wrapper mermaid-block-wrapper" data-mermaid-block data-mermaid-state="pending">' +
          '<div class="code-block-header">' +
          '<span class="code-block-lang">MERMAID</span>' +
          '<span class="mermaid-block-actions">' +
          '<button type="button" class="mermaid-expand-button" data-cccc-mermaid-expand aria-label="' +
          escapedExpandDiagram +
          '" title="' +
          escapedExpandDiagram +
          '" hidden>' +
          '<span class="pointer-events-none">' +
          expandIconMarkup +
          '</span><span class="pointer-events-none hidden sm:inline">' +
          escapedExpand +
          "</span></button>" +
          '<button type="button" class="mermaid-view-toggle" data-cccc-mermaid-toggle aria-pressed="false" data-view-source-label="' +
          escapedViewSource +
          '" data-view-diagram-label="' +
          escapedViewDiagram +
          '">' +
          escapedViewSource +
          "</button>" +
          copyButton +
          "</span>" +
          "</div>" +
          '<div class="mermaid-status" data-cccc-mermaid-status role="status">' +
          escapedRendering +
          "</div>" +
          '<div class="mermaid-render-target" data-cccc-mermaid-target role="button" tabindex="0" aria-label="' +
          escapedExpandDiagram +
          '" title="' +
          escapedExpandDiagram +
          '"></div>' +
          '<div class="mermaid-error" data-cccc-mermaid-error role="alert" hidden data-render-failed="' +
          escapedRenderFailed +
          '" data-too-large="' +
          escapedTooLarge +
          '"></div>' +
          '<pre class="mermaid-source" data-cccc-mermaid-source hidden><code class="language-mermaid">' +
          escaped +
          "</code></pre>" +
          "</div>"
        );
      }

      // Render the full fence here so markdown-it does not wrap custom markup in another pre/code pair.
      return (
        '<div class="code-block-wrapper relative group">' +
        '<div class="code-block-header flex items-center justify-between">' +
        '<span class="code-block-lang uppercase">' +
        escapedLang +
        "</span>" +
        copyButton +
        "</div>" +
        '<pre><code class="language-' +
        escapedLang +
        '">' +
        escaped +
        "</code></pre>" +
        "</div>"
      );
    };
    return instance;
  }, [
    enableMermaid,
    labels.copy,
    labels.copied,
    labels.expand,
    labels.expandDiagram,
    labels.renderFailed,
    labels.rendering,
    labels.tooLarge,
    labels.viewDiagram,
    labels.viewSource,
  ]);

  const htmlContent = useMemo(() => {
    return md.render(content || "");
  }, [md, content]);
  // Mermaid renders into this subtree imperatively. Keep the prop object stable so unrelated parent
  // renders do not make React replace the subtree and reset completed diagrams back to "pending".
  const renderedHtml = useMemo(() => ({ __html: htmlContent }), [htmlContent]);

  const openMermaidPreview = useCallback((trigger: HTMLElement) => {
    const block = trigger.closest<HTMLElement>("[data-mermaid-block]");
    if (!block || block.dataset.mermaidState !== "rendered") return;
    const target = block.querySelector<HTMLElement>("[data-cccc-mermaid-target]");
    const svg = target?.querySelector<SVGSVGElement>("svg");
    const copyButton = block.querySelector<HTMLButtonElement>("button[data-cccc-markdown-copy]");
    if (!target || !svg || !copyButton) return;

    let source = "";
    try {
      source = decodeURIComponent(copyButton.dataset.code || "");
    } catch {
      return;
    }
    const naturalWidth = Number(svg.viewBox?.baseVal?.width || 0);
    setMermaidPreview({
      svg: target.innerHTML,
      source,
      naturalWidth: Number.isFinite(naturalWidth) && naturalWidth > 0 ? naturalWidth : 0,
    });
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !enableMermaid) return;
    return activateMermaidBlocks(container, {
      theme: isDark || invertText ? "dark" : "default",
      renderFailedLabel: labels.renderFailed,
      tooLargeLabel: labels.tooLarge,
    });
  }, [enableMermaid, htmlContent, invertText, isDark, labels.renderFailed, labels.tooLarge]);

  // Use event delegation because the Markdown tree is produced as HTML.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleClick = async (e: MouseEvent) => {
      const expand = (e.target as HTMLElement).closest<HTMLButtonElement>(
        "button[data-cccc-mermaid-expand]",
      );
      if (expand) {
        e.preventDefault();
        e.stopPropagation();
        openMermaidPreview(expand);
        return;
      }

      const toggle = (e.target as HTMLElement).closest<HTMLButtonElement>(
        "button[data-cccc-mermaid-toggle]",
      );
      if (toggle) {
        e.preventDefault();
        e.stopPropagation();
        toggleMermaidView(toggle);
        return;
      }

      const target = (e.target as HTMLElement).closest<HTMLElement>("[data-cccc-mermaid-target]");
      if (target) {
        e.preventDefault();
        e.stopPropagation();
        openMermaidPreview(target);
        return;
      }

      const button = (e.target as HTMLElement).closest<HTMLButtonElement>(
        "button[data-cccc-markdown-copy]",
      );
      if (!button) return;

      e.preventDefault();
      e.stopPropagation();

      const code = decodeURIComponent(button.getAttribute("data-code") || "");
      if (!code) {
        console.error("No code found in data-code attribute");
        return;
      }
      try {
        const copied = await copyTextToClipboard(code);
        if (!copied) throw new Error("copy failed");
        // Toggle state through CSS classes to avoid React DOM sync issues from innerHTML edits.
        button.classList.add("copied", "pointer-events-none");
        const copyIcon = button.querySelector(".copy-icon");
        const copyText = button.querySelector(".copy-text");
        const copiedIcon = button.querySelector(".copied-icon");
        const copiedText = button.querySelector(".copied-text");
        if (copyIcon) copyIcon.classList.add("hidden");
        if (copyText) copyText.classList.add("hidden");
        if (copiedIcon) copiedIcon.classList.remove("hidden");
        if (copiedText) copiedText.classList.remove("hidden");

        setTimeout(() => {
          button.classList.remove("copied", "pointer-events-none");
          if (copyIcon) copyIcon.classList.remove("hidden");
          if (copyText) copyText.classList.remove("hidden");
          if (copiedIcon) copiedIcon.classList.add("hidden");
          if (copiedText) copiedText.classList.add("hidden");
        }, 2000);
      } catch (err) {
        console.error("Failed to copy code:", err);
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      const target = (e.target as HTMLElement).closest<HTMLElement>("[data-cccc-mermaid-target]");
      if (!target) return;
      e.preventDefault();
      e.stopPropagation();
      openMermaidPreview(target);
    };

    container.addEventListener("click", handleClick);
    container.addEventListener("keydown", handleKeyDown);
    return () => {
      container.removeEventListener("click", handleClick);
      container.removeEventListener("keydown", handleKeyDown);
    };
  }, [htmlContent, openMermaidPreview]);

  return (
    <>
      <div
        ref={containerRef}
        className={classNames(
          "markdown-body prose max-w-none prose-sm",
          isDark || invertText ? "prose-invert" : "",
          "[&_p]:m-0 [&_ul]:my-1 [&_ol]:my-1",
          "[&_a]:![color:inherit] [&_a]:underline",
          className,
        )}
        style={{ color: "inherit" }}
        dangerouslySetInnerHTML={renderedHtml}
      />
      {mermaidPreview && (
        <MermaidPreviewDialog
          preview={mermaidPreview}
          labels={labels}
          closeLabel={labels.close}
          onClose={() => setMermaidPreview(null)}
        />
      )}
    </>
  );
}
