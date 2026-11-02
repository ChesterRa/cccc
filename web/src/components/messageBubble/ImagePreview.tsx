import { GraphicViewer } from "../viewer/GraphicViewer";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { MESSAGE_IMAGE_PREVIEW_HEIGHT_PX } from "./imageLayout";
import { ImagePreviewFailure } from "./ImagePreviewFailure";
import { MediaLightbox } from "./MediaLightbox";

const IMAGE_LOAD_ERROR_CACHE = new Set<string>();
// The transparency grid is drawn in the panel tones, and the picture shows as it is:
// no shadow and no contrast lift.
const IMAGE_CANVAS_STYLE = {
  backgroundColor: "var(--lcd-panel)",
  backgroundImage:
    "linear-gradient(45deg, var(--lcd-panel-2) 25%, transparent 25%, transparent 75%, var(--lcd-panel-2) 75%), linear-gradient(45deg, var(--lcd-panel-2) 25%, transparent 25%, transparent 75%, var(--lcd-panel-2) 75%)",
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
  const [resolvedSvgType, setResolvedSvgType] = useState(false);
  const [isResolvingSvg, setIsResolvingSvg] = useState<boolean>(isSvg);
  const [displaySrc, setDisplaySrc] = useState<string>(isSvg ? "" : href);
  const { t } = useTranslation("chat");
  const isGridLayout = layout === "grid";
  const previewHeight = MESSAGE_IMAGE_PREVIEW_HEIGHT_PX[layout];

  useEffect(() => {
    let cancelled = false;
    let objectUrl = "";

    setLoadError(false);
    setResolvedSvgType(false);
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
        // The source may have changed while the response body was loading.
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        // The name can end in .svg while the server identifies raster bytes.
        setResolvedSvgType(blob.type.split(";", 1)[0].trim().toLowerCase() === "image/svg+xml");
        setResolvedHref(objectUrl);
        setIsResolvingSvg(false);
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
        data-image-frame
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
            data-image-canvas
          >
            {alt}
          </div>
        ) : (
          <img
            src={displaySrc || resolvedHref || href}
            alt={alt}
            data-image-canvas
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
            style={isSvg ? undefined : IMAGE_CANVAS_STYLE}
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

      <MediaLightbox
        open={isLightboxVisible}
        onClose={() => setIsLightboxOpen(false)}
        dialogLabel={t("imagePreviewDialog")}
        title={alt}
        downloadHref={downloadHref}
        downloadName={downloadName}
        image
      >
        <GraphicViewer
          src={displaySrc || resolvedHref || href}
          alt={alt}
          isSvg={resolvedSvgType && (displaySrc || resolvedHref || href) === resolvedHref}
          onBlankClick={() => setIsLightboxOpen(false)}
        />
      </MediaLightbox>
    </>
  );
}
