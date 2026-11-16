import { Code, Eye } from "lucide-react";
import { useEffect, useMemo, useState, type MouseEvent as ReactMouseEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { DocumentPreviewKind } from "../../utils/messageAttachments";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { scrollWorkspaceFragment } from "../workspace/workspaceFragment";
import { absoluteDocumentUrl } from "../workspace/workspacePreview";
import { preserveStaticHtmlAlignment, staticWorkspaceHtml } from "../workspace/workspaceHtml";
import { PdfPreview as EmbeddedPdf } from "../viewer/PdfPreview";
import { LcdIcon } from "../Icons";
import { toolbarIconKey } from "../ui/toolbarKey";
import {
  HTML_PREVIEW_BYTES,
  DOCUMENT_PREVIEW_BYTES,
  isPdfDocument,
  readDocumentText,
  type DocumentText,
} from "./documentText";
import { isModifiedClick } from "./messageMedia";

type TextState = DocumentText | { status: "loading" } | { status: "failed" };

const statusClass = "px-4 py-6 text-center text-sm text-[var(--color-text-secondary)]";

// A file on its own has no folder to resolve a relative address against.
const resolveUrl = (href: string, target: "image" | "link") =>
  absoluteDocumentUrl(href, target) ?? "";

function followLink(event: ReactMouseEvent<HTMLElement> | MouseEvent) {
  const anchor = (event.target as Element | null)?.closest("a[href], area[href]");
  if (!anchor || isModifiedClick(event)) return;
  event.preventDefault();
  const href = absoluteDocumentUrl(anchor.getAttribute("href") || "", "link");
  if (href?.startsWith("#") && event.currentTarget)
    scrollWorkspaceFragment(event.currentTarget as HTMLElement | Document, href);
  else if (href) window.open(href, "_blank", "noopener,noreferrer");
}

function HtmlPreview({ text, title }: { text: string; title: string }) {
  const [doc, setDoc] = useState<Document | null>(null);
  const html = useMemo(
    () =>
      staticWorkspaceHtml(
        text,
        (href, target) => {
          const url = resolveUrl(href, target);
          return target === "link" || url.startsWith("data:") ? url : "";
        },
        null,
      ),
    [text],
  );
  useEffect(() => {
    if (!doc) return;
    const restoreAlignment = preserveStaticHtmlAlignment(doc);
    doc.addEventListener("click", followLink);
    return () => {
      doc.removeEventListener("click", followLink);
      restoreAlignment();
    };
  }, [doc]);
  // The parent handles links; the frame must never allow scripts.
  return (
    <iframe
      title={title}
      srcDoc={html}
      sandbox="allow-same-origin"
      data-document-html-preview
      className="min-h-0 w-full flex-1 border-0 bg-white"
      onLoad={(event) => setDoc(event.currentTarget.contentDocument)}
    />
  );
}

function PdfPreview({
  href,
  title,
  failedText,
}: {
  href: string;
  title: string;
  failedText: string;
}) {
  const { t } = useTranslation(["chat", "common"]);
  const [state, setState] = useState<{ href: string; status: "checking" | "ready" | "failed" }>({
    href,
    status: "checking",
  });
  useEffect(() => {
    const controller = new AbortController();
    setState({ href, status: "checking" });
    isPdfDocument(href, controller.signal).then(
      (pdf) => {
        if (!controller.signal.aborted) setState({ href, status: pdf ? "ready" : "failed" });
      },
      () => {
        if (!controller.signal.aborted) setState({ href, status: "failed" });
      },
    );
    return () => controller.abort();
  }, [href]);

  if (state.href !== href || state.status === "checking")
    return <p className={statusClass}>{t("common:loading")}</p>;
  if (state.status === "failed")
    return (
      <p role="alert" className={statusClass}>
        {failedText}
      </p>
    );
  return (
    <EmbeddedPdf href={href} title={title} hint={t("documentPdfHint")} failedText={failedText} />
  );
}

function TextPreview({
  kind,
  href,
  title,
  isDark,
  failedText,
  actionsTarget,
  toggleButtonClassName = toolbarIconKey,
  toggleIconClassName = "h-3.5 w-3.5",
}: {
  kind: "markdown" | "html" | "text";
  href: string;
  title: string;
  isDark: boolean;
  failedText: string;
  actionsTarget?: HTMLElement | null;
  toggleButtonClassName?: string;
  toggleIconClassName?: string;
}) {
  const { t } = useTranslation(["chat", "common"]);
  const [state, setState] = useState<TextState>({ status: "loading" });
  const [showSource, setShowSource] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    setState({ status: "loading" });
    readDocumentText(
      href,
      controller.signal,
      kind === "html" ? HTML_PREVIEW_BYTES : DOCUMENT_PREVIEW_BYTES,
    ).then(
      (result) => {
        if (!controller.signal.aborted) setState(result);
      },
      () => {
        if (!controller.signal.aborted) setState({ status: "failed" });
      },
    );
    return () => controller.abort();
  }, [href, kind]);

  if (state.status === "loading") return <p className={statusClass}>{t("common:loading")}</p>;
  if (state.status === "failed")
    return (
      <p role="alert" className={statusClass}>
        {failedText}
      </p>
    );
  if (state.status === "binary") return <p className={statusClass}>{t("workspaceBinary")}</p>;
  const htmlTruncated = kind === "html" && state.truncated;
  const source = showSource || kind === "text" || htmlTruncated;
  const canToggle = kind !== "text" && !htmlTruncated;
  const toggle = canToggle ? (
    <button
      type="button"
      onClick={() => setShowSource(!showSource)}
      title={t(showSource ? "workspacePreview" : "workspaceViewSource")}
      aria-label={t(showSource ? "workspacePreview" : "workspaceViewSource")}
      className={`${toggleButtonClassName} shrink-0`}
      data-lcd-key
    >
      <LcdIcon
        icon={showSource ? Eye : Code}
        glyph={showSource ? "eye" : "code"}
        className={toggleIconClassName}
      />
    </button>
  ) : null;
  return (
    <div className="flex h-full min-h-0 flex-col" data-document-preview>
      {actionsTarget ? createPortal(toggle, actionsTarget) : null}
      {toggle && actionsTarget === undefined && (
        <div className="flex shrink-0 justify-end border-b border-[var(--glass-border-subtle)] px-2 py-1.5">
          {toggle}
        </div>
      )}
      {kind === "html" && !source && (
        <p className="shrink-0 border-b border-[var(--glass-border-subtle)] px-3 py-2 text-xs text-[var(--color-text-secondary)]">
          {t("documentHtmlStatic")}
        </p>
      )}
      {(state.truncated || (source && state.sourceTruncated)) && (
        <p role="status" className="shrink-0 px-4 pt-3 text-xs text-[var(--color-text-secondary)]">
          {t(htmlTruncated ? "documentHtmlTruncated" : "documentPreviewTruncated")}
        </p>
      )}
      {kind === "html" && !source ? (
        <HtmlPreview text={state.text} title={title} />
      ) : (
        <div className="min-h-0 flex-1 overflow-auto" tabIndex={0} onClick={followLink}>
          {kind === "markdown" && !source ? (
            <div className="mx-auto max-w-3xl px-4 py-5">
              <LazyMarkdownRenderer
                content={state.text}
                isDark={isDark}
                enableMermaid
                resolveUrl={resolveUrl}
                className="break-words [overflow-wrap:anywhere]"
                fallback={<pre className="whitespace-pre-wrap">{state.text}</pre>}
              />
            </div>
          ) : (
            <pre className="m-0 whitespace-pre-wrap break-words p-4 font-mono text-xs leading-5 text-[var(--color-text-primary)] [overflow-wrap:anywhere]">
              {state.sourceText}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

/** A file read in the page. It loads when shown and stops loading when closed. */
export function DocumentPreview({
  kind,
  href,
  title,
  isDark,
  failedText,
  actionsTarget,
  toggleButtonClassName,
  toggleIconClassName,
}: {
  kind: DocumentPreviewKind;
  href: string;
  title: string;
  isDark: boolean;
  failedText: string;
  /** A visible host's title-bar slot; null keeps a hidden tab's control out of it. */
  actionsTarget?: HTMLElement | null;
  /** Match the host toolbar's existing key and icon sizes. */
  toggleButtonClassName?: string;
  toggleIconClassName?: string;
}) {
  if (kind === "pdf") return <PdfPreview href={href} title={title} failedText={failedText} />;
  return (
    <TextPreview
      key={JSON.stringify([kind, href])}
      kind={kind}
      href={href}
      title={title}
      isDark={isDark}
      failedText={failedText}
      toggleButtonClassName={toggleButtonClassName}
      toggleIconClassName={toggleIconClassName}
      actionsTarget={actionsTarget}
    />
  );
}
