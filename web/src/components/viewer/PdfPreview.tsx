import { Component, lazy, Suspense, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMediaQuery } from "../../hooks/useMediaQuery";

const PdfCanvasViewer = lazy(() =>
  import("./PdfCanvasViewer").then((module) => ({ default: module.PdfCanvasViewer })),
);

// A failed reader must not unmount the surrounding editor and its unsaved drafts.
class PdfErrorBoundary extends Component<
  { children: ReactNode; fallback: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

/** Keep the native desktop viewer, and use a page renderer where embedding is unreliable. */
export function PdfPreview({
  href,
  title,
  hint,
  failedText,
}: {
  href: string;
  title: string;
  hint?: string;
  failedText: string;
}) {
  const { t } = useTranslation("common");
  const touch = useMediaQuery("(pointer: coarse)");
  const useCanvas = touch || navigator.pdfViewerEnabled === false;
  if (useCanvas) {
    return (
      <PdfErrorBoundary
        key={href}
        fallback={
          <p role="alert" className="p-4 text-sm">
            {failedText}
          </p>
        }
      >
        <Suspense fallback={<p className="p-4 text-sm">{t("loading")}</p>}>
          <PdfCanvasViewer href={href} title={title} failedText={failedText} />
        </Suspense>
      </PdfErrorBoundary>
    );
  }
  return (
    <div className="flex h-full min-h-0 flex-1 flex-col">
      {hint ? (
        <p className="shrink-0 px-4 py-2 text-xs text-[var(--color-text-secondary)]">{hint}</p>
      ) : null}
      <iframe title={title} src={href} className="min-h-0 w-full flex-1 border-0 bg-white" />
    </div>
  );
}
