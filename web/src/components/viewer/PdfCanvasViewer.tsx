import { useVirtualizer } from "@tanstack/react-virtual";
import { Maximize, Minus, Plus } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getDocument,
  GlobalWorkerOptions,
  version,
  type PDFDocumentProxy,
  type PDFPageProxy,
  type RenderTask,
} from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import { LcdIcon } from "../Icons";
import { buttonVariants } from "../ui/button-variants";

GlobalWorkerOptions.workerSrc = workerUrl;

const GAP = 12;
// Bound canvas memory even for a very long or high-resolution page.
const MAX_CANVAS_PIXELS = 3_000_000;
const MAX_CANVAS_DIMENSION = 4096;

type DocumentState =
  | { status: "loading" }
  | { status: "failed" }
  | { status: "ready"; document: PDFDocumentProxy; pageRatio: number };

function assetRoot() {
  const base = import.meta.env.BASE_URL;
  return import.meta.env.DEV
    ? `${base}node_modules/pdfjs-dist/`
    : `${base}assets/pdfjs/${version}/`;
}

function pageFromHref(href: string) {
  try {
    const hash = new URL(href, window.location.href).hash.slice(1);
    const page = new URLSearchParams(hash).get("page") || "";
    const number = /^[1-9]\d*$/.test(page) ? Number(page) : 1;
    return Number.isSafeInteger(number) ? number - 1 : 0;
  } catch {
    return 0;
  }
}

/** One mounted page owns one canvas and cancels its work when resized or removed. */
function PdfPage({
  document,
  number,
  width,
  estimatedHeight,
  title,
  failedText,
}: {
  document: PDFDocumentProxy;
  number: number;
  width: number;
  estimatedHeight: number;
  title: string;
  failedText: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [height, setHeight] = useState(estimatedHeight);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let page: PDFPageProxy | null = null;
    let render: RenderTask | null = null;
    const canvas = canvasRef.current;
    if (!canvas || width <= 0) return;
    setFailed(false);
    void document
      .getPage(number)
      .then(async (loaded) => {
        if (cancelled) return;
        page = loaded;
        const initial = loaded.getViewport({ scale: 1 });
        const viewport = loaded.getViewport({ scale: width / initial.width });
        setHeight(Math.ceil(viewport.height));
        const pixels = Math.max(1, viewport.width * viewport.height);
        const scale = Math.min(
          window.devicePixelRatio || 1,
          2,
          Math.sqrt(MAX_CANVAS_PIXELS / pixels),
          MAX_CANVAS_DIMENSION / Math.max(viewport.width, viewport.height),
        );
        canvas.width = Math.max(1, Math.floor(viewport.width * scale));
        canvas.height = Math.max(1, Math.floor(viewport.height * scale));
        canvas.style.width = `${viewport.width}px`;
        canvas.style.height = `${viewport.height}px`;
        render = loaded.render({ canvas, viewport, transform: [scale, 0, 0, scale, 0, 0] });
        await render.promise;
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      render?.cancel();
      // The next render uses a new canvas, so cancellation cannot race it.
      canvas.width = 0;
      canvas.height = 0;
      if (render) void render.promise.catch(() => undefined).finally(() => page?.cleanup());
      else page?.cleanup();
    };
  }, [document, number, width]);

  return (
    <div className="relative mx-auto bg-white shadow-sm" style={{ width, height }}>
      <canvas
        key={width}
        ref={canvasRef}
        role="img"
        aria-label={`${title} · ${number}`}
        data-pdf-page={number}
      />
      {failed ? (
        <p role="alert" className="absolute inset-0 bg-white p-3 text-sm text-slate-800">
          {failedText}
        </p>
      ) : null}
    </div>
  );
}

/** Scroll through a PDF in the page; only visible pages and their neighbors are rendered. */
export function PdfCanvasViewer({
  href,
  title,
  failedText,
}: {
  href: string;
  title: string;
  failedText: string;
}) {
  const { t } = useTranslation(["chat", "common"]);
  const [state, setState] = useState<DocumentState>({ status: "loading" });
  const [width, setWidth] = useState(0);
  const [zoom, setZoom] = useState(1);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lastPage = useRef(pageFromHref(href));
  const lastLayout = useRef({ visible: false, pageWidth: 0 });

  useEffect(() => {
    const root = assetRoot();
    const task = getDocument({
      url: href,
      cMapUrl: `${root}cmaps/`,
      cMapPacked: true,
      standardFontDataUrl: `${root}standard_fonts/`,
      wasmUrl: `${root}wasm/`,
      iccUrl: `${root}iccs/`,
    });
    let cancelled = false;
    setState({ status: "loading" });
    void task.promise
      .then(async (document) => {
        if (!document.numPages) throw new Error("PDF has no pages");
        const first = await document.getPage(1);
        const viewport = first.getViewport({ scale: 1 });
        if (!cancelled)
          setState({ status: "ready", document, pageRatio: viewport.height / viewport.width });
      })
      .catch(() => {
        if (!cancelled) setState({ status: "failed" });
      });
    return () => {
      cancelled = true;
      void task.destroy().catch(() => undefined);
    };
  }, [href]);

  useEffect(() => {
    const node = scrollRef.current;
    if (!node) return;
    const measure = () => setWidth(Math.floor(node.clientWidth));
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const document = state.status === "ready" ? state.document : null;
  const pageWidth = Math.max(1, Math.floor(Math.max(1, width - 2 * GAP) * zoom));
  const estimatedHeight = Math.ceil(pageWidth * (state.status === "ready" ? state.pageRatio : 1.4));
  // eslint-disable-next-line react-hooks/incompatible-library
  const pages = useVirtualizer({
    count: document?.numPages ?? 0,
    enabled: width > 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimatedHeight + GAP,
    overscan: 1,
  });
  const visible = pages.getVirtualItems();
  const current = visible.find((item) => item.end > (pages.scrollOffset ?? 0))?.index ?? 0;
  useLayoutEffect(() => {
    const node = scrollRef.current;
    // Hidden, kept-mounted panels report zero dimensions. Do not replace the
    // last page with the empty virtual range while they are out of view.
    if (!document || !node || node.clientWidth <= 0 || node.clientHeight <= 0) {
      lastLayout.current.visible = false;
      return;
    }
    if (!lastLayout.current.visible || lastLayout.current.pageWidth !== pageWidth) {
      lastLayout.current = { visible: true, pageWidth };
      pages.measure();
      pages.scrollToIndex(Math.min(lastPage.current, document.numPages - 1), { align: "start" });
      return;
    }
    lastPage.current = current;
  }, [current, document, pageWidth, pages, width]);
  const button = `${buttonVariants({ variant: "ghost", size: "sm" })} min-w-9 max-sm:min-h-11 max-sm:min-w-11`;

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col" data-pdf-canvas-viewer>
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-x-3 gap-y-1 border-b border-[var(--glass-border-subtle)] p-1 text-xs text-[var(--color-text-secondary)]">
        <span className="tabular-nums">
          {document ? `${current + 1} / ${document.numPages}` : ""}
        </span>
        <div className="flex items-center gap-1">
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!document}
            onClick={() => setZoom(1)}
            title={t("graphicViewer.fit")}
            aria-label={t("graphicViewer.fit")}
          >
            <LcdIcon icon={Maximize} glyph="maximize" size={14} />
            <span>{t("graphicViewer.fit")}</span>
          </button>
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!document || zoom <= 1}
            onClick={() => setZoom((value) => Math.max(1, value / 1.25))}
            aria-label={t("graphicViewer.out")}
            title={t("graphicViewer.out")}
          >
            <LcdIcon icon={Minus} glyph="minus" size={15} />
          </button>
          <span className="min-w-12 text-center tabular-nums">{Math.round(zoom * 100)}%</span>
          <button
            type="button"
            data-lcd-key
            className={button}
            disabled={!document || zoom >= 4}
            onClick={() => setZoom((value) => Math.min(4, value * 1.25))}
            aria-label={t("graphicViewer.in")}
            title={t("graphicViewer.in")}
          >
            <LcdIcon icon={Plus} glyph="plus" size={15} />
          </button>
        </div>
      </div>
      <div
        ref={scrollRef}
        className="relative min-h-0 min-w-0 flex-1 overflow-auto bg-[var(--color-bg-primary)]"
        tabIndex={0}
      >
        {state.status === "loading" ? (
          <p role="status" className="p-4 text-sm text-[var(--color-text-secondary)]">
            {t("common:loading")}
          </p>
        ) : null}
        {state.status === "failed" ? (
          <p role="alert" className="p-4 text-sm text-[var(--color-text-secondary)]">
            {failedText}
          </p>
        ) : null}
        {document && width > 0 ? (
          <div
            className="relative"
            style={{ height: pages.getTotalSize(), width: Math.max(width, pageWidth + 2 * GAP) }}
          >
            {visible.map((item) => (
              <div
                key={item.key}
                data-index={item.index}
                ref={pages.measureElement}
                className="absolute left-0 top-0 w-full"
                style={{ transform: `translateY(${item.start}px)`, paddingBottom: GAP }}
              >
                <PdfPage
                  document={document}
                  number={item.index + 1}
                  width={pageWidth}
                  estimatedHeight={estimatedHeight}
                  title={title}
                  failedText={failedText}
                />
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}
