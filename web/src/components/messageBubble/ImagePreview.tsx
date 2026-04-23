import { FloatingPortal } from "@floating-ui/react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { CloseIcon, ImageIcon } from "../Icons";

const IMAGE_ASPECT_RATIO_CACHE = new Map<string, number>();
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
  alt,
  isSvg,
  isUserMessage,
  isDark,
  layout = "hero",
}: {
  href: string;
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
  const [aspectRatio, setAspectRatio] = useState<number | null>(() => IMAGE_ASPECT_RATIO_CACHE.get(href) ?? null);
  const [displaySrc, setDisplaySrc] = useState<string>(isSvg ? "" : href);
  const { t } = useTranslation("chat");
  const isGridLayout = layout === "grid";
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
      void img.decode().then(finalize).catch(() => {
        void 0;
      });
    }

    return () => {
      cancelled = true;
      img.onload = null;
      img.onerror = null;
    };
  }, [displaySrc, href, resolvedHref]);

  useEffect(() => {
    if (!isLightboxOpen) {
      return undefined;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsLightboxOpen(false);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isLightboxOpen]);

  useEffect(() => {
    const src = displaySrc || resolvedHref || href;
    if (!src || isResolvingSvg || IMAGE_LOAD_ERROR_CACHE.has(href)) {
      return undefined;
    }
    const cachedAspectRatio = IMAGE_ASPECT_RATIO_CACHE.get(href);
    if (typeof cachedAspectRatio === "number" && cachedAspectRatio > 0) {
      return undefined;
    }
    let cancelled = false;
    const img = new Image();
    img.onload = () => {
      if (cancelled) return;
      const width = Number(img.naturalWidth || 0);
      const height = Number(img.naturalHeight || 0);
      if (width > 0 && height > 0) {
        const nextAspectRatio = width / height;
        IMAGE_ASPECT_RATIO_CACHE.set(href, nextAspectRatio);
        setAspectRatio(nextAspectRatio);
      }
    };
    img.onerror = () => {
      if (cancelled) return;
      IMAGE_LOAD_ERROR_CACHE.add(href);
      setLoadError(true);
    };
    img.src = src;
    return () => {
      cancelled = true;
    };
  }, [displaySrc, href, isResolvingSvg, resolvedHref]);

  if (loadError) {
    return (
      <a
        href={href}
        className={classNames(
          "group flex w-full flex-col overflow-hidden rounded-xl border p-2 text-left transition-colors",
          isUserMessage
            ? "border-[rgb(35,36,37)]/24 bg-white/10 text-white hover:bg-white/14"
            : isDark
              ? "border-white/10 bg-slate-900/50 text-slate-300 hover:bg-slate-900/65"
              : "border-[rgba(15,23,42,0.12)] bg-[rgb(238,241,245)] text-[var(--color-text-secondary)] hover:bg-[rgb(232,236,241)]",
        )}
        title={t("download", { name: alt })}
        download
      >
        <div
          className={classNames(
            "flex aspect-[4/3] w-full flex-col items-center justify-center rounded-lg border border-dashed px-4 text-center",
            isUserMessage
              ? "border-white/20 bg-black/10"
              : isDark
                ? "border-white/12 bg-slate-950/70"
                : "border-[rgba(15,23,42,0.14)] bg-white/85",
          )}
        >
          <ImageIcon size={24} className="mb-3 flex-shrink-0 opacity-65" />
          <div className={classNames("text-xs font-semibold", isUserMessage ? "text-white" : "text-[var(--color-text-primary)]")}>
            {t("imagePreviewUnavailable", { defaultValue: "图片预览失败" })}
          </div>
          <div className="mt-1 text-[11px] opacity-75">
            {t("downloadOriginalImage", { defaultValue: "点击下载原图" })}
          </div>
        </div>
        <div className="min-w-0 px-1 pt-2">
          <div className={classNames("truncate text-[11px] font-medium", isUserMessage ? "text-white/88" : "text-[var(--color-text-secondary)]")}>
            {alt}
          </div>
        </div>
      </a>
    );
  }

  return (
    <>
      <button
        type="button"
        className={classNames(
          "group overflow-hidden rounded-xl border transition-colors",
          isSvg ? "block" : "inline-flex w-full",
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
        style={isSvg ? { width: isGridLayout ? "10rem" : "12rem", maxWidth: "100%" } : undefined}
      >
        {isResolvingSvg ? (
          <div
            className={classNames(
              "flex items-center justify-center rounded-lg border px-4 py-6 text-xs",
              isGridLayout ? "min-h-32 min-w-32" : "min-h-28 min-w-28",
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
              "cursor-zoom-in rounded-lg transition-opacity group-hover:opacity-95",
              isSvg
                ? classNames("block h-auto w-full", isGridLayout ? "max-h-40" : "max-h-64 sm:max-h-80")
                : "block h-full w-full object-cover",
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
                    aspectRatio: isGridLayout ? "4 / 3" : (aspectRatio ?? "4 / 3"),
                    maxHeight: isGridLayout ? "11rem" : "20rem",
                    ...rasterCanvasStyle,
                    ...(!isUserMessage && !isDark
                      ? LIGHT_THEME_IMAGE_ENHANCEMENT_STYLE
                      : null),
                  }
            }
            loading={isSvg ? "lazy" : "eager"}
            decoding="async"
            onError={() => {
              IMAGE_LOAD_ERROR_CACHE.add(href);
              setLoadError(true);
            }}
            onLoad={(event) => {
              if (isSvg) return;
              const target = event.currentTarget;
              const width = Number(target.naturalWidth || 0);
              const height = Number(target.naturalHeight || 0);
              if (width > 0 && height > 0) {
                const nextAspectRatio = width / height;
                IMAGE_ASPECT_RATIO_CACHE.set(href, nextAspectRatio);
                setAspectRatio(nextAspectRatio);
              }
            }}
          />
        )}
      </button>

      {isLightboxOpen && (
        <FloatingPortal>
          <div className="fixed inset-0 z-[80] flex items-center justify-center p-3 sm:p-6 animate-fade-in">
            <button
              type="button"
              className={classNames("absolute inset-0", "glass-overlay")}
              onClick={() => setIsLightboxOpen(false)}
              aria-label={t("common:close")}
            />

            <div
              className={classNames(
                "relative z-[81] flex max-h-[90vh] w-full max-w-5xl flex-col overflow-hidden rounded-2xl border shadow-2xl",
                "glass-modal",
              )}
              role="dialog"
              aria-modal="true"
              aria-label={t("imagePreviewDialog")}
              onClick={(event) => event.stopPropagation()}
            >
              <div className={classNames("flex items-center justify-between gap-3 border-b px-4 py-3", "border-[var(--glass-border-subtle)]")}>
                <div className="min-w-0">
                  <p className={classNames("truncate text-sm font-medium", "text-[var(--color-text-primary)]")}>{alt}</p>
                  <p className={classNames("text-xs", "text-[var(--color-text-tertiary)]")}>{t("imagePreviewHint")}</p>
                </div>

                <div className="flex items-center gap-2">
                  <a
                    href={href}
                    download
                    className={classNames(
                      "inline-flex items-center rounded-lg px-3 py-2 text-xs font-medium transition-colors",
                      isDark ? "bg-slate-800 text-slate-100 hover:bg-slate-700" : "bg-gray-100 text-gray-700 hover:bg-gray-200",
                    )}
                    title={t("download", { name: alt })}
                  >
                    {t("download", { name: alt })}
                  </a>

                  <button
                    type="button"
                    onClick={() => setIsLightboxOpen(false)}
                    className={classNames(
                      "inline-flex items-center justify-center rounded-lg p-2 transition-colors",
                      isDark ? "text-slate-300 hover:bg-slate-800 hover:text-slate-100" : "text-gray-500 hover:bg-gray-100 hover:text-gray-700",
                    )}
                    aria-label={t("common:close")}
                  >
                    <CloseIcon size={18} />
                  </button>
                </div>
              </div>

              <div className="flex items-center justify-center overflow-auto p-4 sm:p-6">
                <img
                  src={displaySrc || resolvedHref || href}
                  alt={alt}
                  className="max-h-[75vh] w-auto max-w-full rounded-xl object-contain"
                  style={isUserMessage || isDark ? undefined : LIGHT_THEME_IMAGE_ENHANCEMENT_STYLE}
                />
              </div>
            </div>
          </div>
        </FloatingPortal>
      )}
    </>
  );
}
