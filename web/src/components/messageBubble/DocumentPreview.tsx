import { useEffect, useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import type { DocumentPreviewKind } from "../../utils/messageAttachments";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { scrollWorkspaceFragment } from "../workspace/workspaceFragment";
import { absoluteDocumentUrl } from "../workspace/workspacePreview";
import { PdfPreview as EmbeddedPdf } from "../viewer/PdfPreview";
import { isPdfDocument, readDocumentText, type DocumentText } from "./documentText";
import { isModifiedClick } from "./messageMedia";

type TextState = DocumentText | { status: "loading" } | { status: "failed" };

const statusClass = "px-4 py-6 text-center text-sm text-[var(--color-text-secondary)]";

// A file on its own has no folder to resolve a relative address against.
const resolveUrl = (href: string, target: "image" | "link") =>
  absoluteDocumentUrl(href, target) ?? "";

function followLink(event: MouseEvent<HTMLElement>) {
  const anchor = (event.target as Element | null)?.closest("a[href]");
  if (!anchor || isModifiedClick(event)) return;
  event.preventDefault();
  const href = anchor.getAttribute("href") || "";
  if (href.startsWith("#")) scrollWorkspaceFragment(event.currentTarget, href);
  else if (href) window.open(href, "_blank", "noopener,noreferrer");
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
  isDark,
  failedText,
}: {
  kind: "markdown" | "text";
  href: string;
  isDark: boolean;
  failedText: string;
}) {
  const { t } = useTranslation(["chat", "common"]);
  const [state, setState] = useState<TextState>({ status: "loading" });
  useEffect(() => {
    const controller = new AbortController();
    setState({ status: "loading" });
    readDocumentText(href, controller.signal).then(
      (result) => {
        if (!controller.signal.aborted) setState(result);
      },
      () => {
        if (!controller.signal.aborted) setState({ status: "failed" });
      },
    );
    return () => controller.abort();
  }, [href]);

  if (state.status === "loading") return <p className={statusClass}>{t("common:loading")}</p>;
  if (state.status === "failed")
    return (
      <p role="alert" className={statusClass}>
        {failedText}
      </p>
    );
  if (state.status === "binary") return <p className={statusClass}>{t("workspaceBinary")}</p>;
  return (
    <div className="h-full overflow-auto" tabIndex={0} onClick={followLink} data-document-preview>
      {state.truncated && (
        <p role="status" className="px-4 pt-3 text-xs text-[var(--color-text-secondary)]">
          {t("documentPreviewTruncated")}
        </p>
      )}
      {kind === "markdown" ? (
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
          {state.text}
        </pre>
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
}: {
  kind: DocumentPreviewKind;
  href: string;
  title: string;
  isDark: boolean;
  failedText: string;
}) {
  if (kind === "pdf") return <PdfPreview href={href} title={title} failedText={failedText} />;
  return <TextPreview kind={kind} href={href} isDark={isDark} failedText={failedText} />;
}
