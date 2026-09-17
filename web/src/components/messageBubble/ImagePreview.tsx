import { buttonVariants } from "../ui/button-variants";
import { GraphicViewer } from "../viewer/GraphicViewer";
import { useModalA11y } from "../../hooks/useModalA11y";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { FloatingPortal } from "@floating-ui/react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { CloseIcon, DownloadIcon } from "../Icons";
import { MESSAGE_IMAGE_PREVIEW_HEIGHT_PX } from "./imageLayout";
import { ImagePreviewFailure } from "./ImagePreviewFailure";

const IMAGE_LOAD_ERROR_CACHE = new Set<string>();
const LIGHT_THEME_IMAGE_ENHANCEMENT_STYLE = {
  filter: "contrast(1.12) brightness(0.985) saturate(1.01)",
  boxShadow:
    "0 10px 24px -18px rgba(15,23,42,0.24), 0 0 0 1px rgba(15,23,42,0.08), inset 0 1px 0 rgba(255,255,255,0.82)",
} as const;
const LIGHT_IMAGE_CANVAS_STYLE = {
  backgroundColor: "rgb(236, 239, 243)",
  backgroundImage:
    "linear-gradient(45deg, rgba(15,23,42,0.06) 25%, transparent 25%, transparent 75%, rgba(15,23,42,0.06) 75%), linear-gradient(45deg, rgba(15,23,42,0.06) 25%, transparent 25%, transparent 75%, rgba(15,23,42,0.06) 75%)",
  backgroundPosition: "0 0, 8px 8px",
  backgroundSize: "16px 16px",
} as const;
const DARK_IMAGE_CANVAS_STYLE = {
  backgroundColor: "rgb(22, 24, 29)",
  backgroundImage:
    "linear-gradient(45deg, rgba(255,255,255,0.035) 25%, transparent 25%, transparent 75%, rgba(255,255,255,0.035) 75%), linear-gradient(45deg, rgba(255,255,255,0.035) 25%, transparent 25%, transparent 75%, rgba(255,255,255,0.035) 75%)",
  backgroundPosition: "0 0, 8px 8px",
  backgroundSize: "16px 16px",
} as const;

export function ImagePreview({
  href,
  downloadHref,
  downloadName,
  alt,
  isSvg,
  isUserMessage,
  isDark,
  layout = "hero",
}: {
  href: string;
  downloadHref: string;
  downloadName: string;
  alt: string;
  isSvg: boolean;
  isUserMessage: boolean;
  isDark: boolean;
  layout?: "hero" | "grid";
}) {
  const [loadError, setLoadError] = useState(() => IMAGE_LOAD_ERROR_CACHE.has(href));
  const [isLightboxOpen, setIsLightboxOpen] = useState(false);
  const [resolvedHref, setResolvedHref] = useState<string>(isSvg ? "" : href);
  const [isResolvingSvg, setIsResolvingSvg] = useState<boolean>(isSvg);
  const [displaySrc, setDisplaySrc] = useState<string>(isSvg ? "" : href);
  const { t } = useTranslation("chat");
  const isGridLayout = layout === "grid";
  const previewHeight = MESSAGE_IMAGE_PREVIEW_HEIGHT_PX[layout];
  const rasterCanvasStyle = isDark ? DARK_IMAGE_CANVAS_STYLE : LIGHT_IMAGE_CANVAS_STYLE;

  useEffect(() => {
    let cancelled = false;
    let objectUrl = "";

    setLoadError(false);
    if (!isSvg || href.startsWith("blob:") || href.startsWith("data:")) {
      setResolvedHref(href);
      setIsResolvingSvg(false);
      return undefined;
    }

    setIsResolvingSvg(true);

    void (async () => {
      try {
        const resp = await fetch(href, { credentials: "same-origin" });
        if (!resp.ok) {
          throw new Error(`svg_fetch_failed:${resp.status}`);
        }
        const blob = await resp.blob();
        objectUrl = URL.createObjectURL(blob);
        if (!cancelled) {
          setResolvedHref(objectUrl);
          setIsResolvingSvg(false);
        }
      } catch {
        if (!cancelled) {
          setLoadError(true);
          setIsResolvingSvg(false);
        }
      }
    })();

    return () => {
      cancelled = true;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [href, isSvg]);

  useEffect(() => {
    const nextSrc = resolvedHref || href;
    if (!nextSrc) {
      setDisplaySrc("");
      return undefined;
    }
    if (nextSrc === displaySrc) {
      return undefined;
    }
    if (!displaySrc || nextSrc.startsWith("blob:") || nextSrc.startsWith("data:")) {
      setDisplaySrc(nextSrc);
      return undefined;
    }

    let cancelled = false;
    const img = new Image();
    const finalize = () => {
      if (cancelled) return;
      setDisplaySrc(nextSrc);
    };
    const fail = () => {
      if (cancelled) return;
      IMAGE_LOAD_ERROR_CACHE.add(href);
      setLoadError(true);
    };

    img.onload = finalize;
    img.onerror = fail;
    img.src = nextSrc;
    if (typeof img.decode === "function") {
      void img
        .decode()
        .then(finalize)
        .catch(() => {
          void 0;
        });
    }

    return () => {
      cancelled = true;
      img.onload = null;
      img.onerror = null;
    };
  }, [displaySrc, href, resolvedHref]);

  const isLightboxVisible = isLightboxOpen && !loadError;
  const { modalRef } = useModalA11y(isLightboxVisible, () => setIsLightboxOpen(false));
  useEffect(() => {
    if (loadError) setIsLightboxOpen(false);
  }, [loadError]);

  if (loadError) {
    return (
      <ImagePreviewFailure
        href={downloadHref}
        downloadName={downloadName}
        alt={alt}
        isUserMessage={isUserMessage}
        isDark={isDark}
        layout={layout}
        height={previewHeight}
        title={t("download", { name: alt })}
        unavailableLabel={t("imagePreviewUnavailable", {
          defaultValue: "Image preview unavailable",
        })}
        openOriginalLabel={t("downloadOriginalImage", { defaultValue: "Open the original image" })}
      />
    );
  }

  return (
    <>
      <button
        type="button"
        className={classNames(
          "group inline-flex w-full overflow-hidden rounded-xl border transition-colors",
          isGridLayout ? "p-1.5" : "p-2",
          isUserMessage
            ? "border-[rgb(35,36,37)]/14 bg-white/10 hover:bg-white/14"
            : isDark
              ? "border-white/10 bg-slate-900/45 hover:bg-slate-900/55"
              : "border-[rgba(15,23,42,0.12)] bg-[rgb(238,241,245)] hover:bg-[rgb(232,236,241)]",
        )}
        onClick={() => setIsLightboxOpen(true)}
        aria-label={t("openImagePreview", { name: alt })}
        title={t("openImagePreview", { name: alt })}
        disabled={isResolvingSvg}
        style={{ height: previewHeight }}
      >
        {isResolvingSvg ? (
          <div
            className={classNames(
              "flex h-full w-full items-center justify-center rounded-lg border px-4 text-xs",
              isUserMessage
                ? "border-[rgb(35,36,37)]/40 bg-[rgb(35,36,37)]/16 text-white"
                : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]",
            )}
          >
            {alt}
          </div>
        ) : (
          <img
            src={displaySrc || resolvedHref || href}
            alt={alt}
            className={classNames(
              "block h-full w-full cursor-zoom-in rounded-lg object-contain transition-opacity group-hover:opacity-95",
              isSvg
                ? null
                : isUserMessage
                  ? "bg-white"
                  : isDark
                    ? "bg-slate-950/80"
                    : "bg-white shadow-[0_10px_24px_-18px_rgba(15,23,42,0.22),0_0_0_1px_rgba(15,23,42,0.08)]",
            )}
            style={
              isSvg
                ? undefined
                : {
                    ...rasterCanvasStyle,
                    ...(!isUserMessage && !isDark ? LIGHT_THEME_IMAGE_ENHANCEMENT_STYLE : null),
                  }
            }
            loading={isSvg ? "lazy" : "eager"}
            decoding="async"
            onError={(event) => {
              // A previous preview can remain visible while the next source is preloaded.
              if (event.currentTarget.getAttribute("src") !== (resolvedHref || href)) return;
              IMAGE_LOAD_ERROR_CACHE.add(href);
              setLoadError(true);
            }}
          />
        )}
      </button>

      {isLightboxVisible && (
        <FloatingPortal>
          <div className="fixed inset-0 z-[80] flex items-center justify-center sm:p-4 animate-fade-in">
            <button
              type="button"
              className={classNames("absolute inset-0", "glass-overlay")}
              onClick={() => setIsLightboxOpen(false)}
              aria-label={t("common:close")}
            />

            <div
              className={classNames(
                "relative z-[81] flex h-[100dvh] sm:h-[90dvh] w-full max-w-5xl flex-col overflow-hidden rounded-2xl border shadow-2xl",
                "glass-modal",
              )}
              ref={modalRef}
              role="dialog"
              aria-modal="true"
              aria-label={t("imagePreviewDialog")}
              onClick={(event) => event.stopPropagation()}
            >
              <div
                className={classNames(
                  "flex items-center justify-between gap-3 border-b px-4 py-3",
                  "border-[var(--glass-border-subtle)]",
                )}
              >
                <div className="min-w-0">
                  <p
                    className={classNames(
                      "truncate text-sm font-medium",
                      "text-[var(--color-text-primary)]",
                    )}
                  >
                    {alt}
                  </p>
                  <p className={classNames("text-xs", "text-[var(--color-text-tertiary)]")}>
                    {t("imagePreviewHint")}
                  </p>
                </div>

                <div className="flex shrink-0 items-center gap-2">
                  <AuthenticatedDownloadLink
                    href={downloadHref}
                    download={downloadName}
                    className={`${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11`}
                    title={t("download", { name: alt })}
                    aria-label={t("download", { name: alt })}
                  >
                    <DownloadIcon size={18} aria-hidden="true" />
                  </AuthenticatedDownloadLink>

                  <button
                    type="button"
                    onClick={() => setIsLightboxOpen(false)}
                    className={`${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11`}
                    aria-label={t("common:close")}
                  >
                    <CloseIcon size={18} />
                  </button>
                </div>
              </div>

              <div className="min-h-0 flex-1">
                <GraphicViewer src={displaySrc || resolvedHref || href} alt={alt} />
              </div>
            </div>
          </div>
        </FloatingPortal>
      )}
    </>
  );
}
