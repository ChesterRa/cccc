import { classNames } from "../../utils/classNames";
import { ImageIcon } from "../Icons";

export function ImagePreviewFailure({
  href,
  downloadName,
  alt,
  isUserMessage,
  isDark,
  layout,
  height,
  title,
  unavailableLabel,
  openOriginalLabel,
}: {
  href: string;
  downloadName: string;
  alt: string;
  isUserMessage: boolean;
  isDark: boolean;
  layout: "hero" | "grid";
  height: number;
  title: string;
  unavailableLabel: string;
  openOriginalLabel: string;
}) {
  const isGridLayout = layout === "grid";

  return (
    <a
      href={href}
      className={classNames(
        "group flex w-full flex-col overflow-hidden rounded-xl border p-2 text-left transition-colors",
        isUserMessage
          ? isDark
            ? "border-[rgb(35,36,37)]/24 bg-white/10 text-white hover:bg-white/14"
            : "border-[rgba(15,23,42,0.18)] bg-white text-[rgb(35,36,37)] shadow-[0_8px_22px_-18px_rgba(15,23,42,0.34)] hover:bg-[rgb(248,250,252)]"
          : isDark
            ? "border-white/10 bg-slate-900/50 text-slate-300 hover:bg-slate-900/65"
            : "border-[rgba(15,23,42,0.12)] bg-[rgb(238,241,245)] text-[var(--color-text-secondary)] hover:bg-[rgb(232,236,241)]",
      )}
      style={{ height }}
      title={title}
      download={downloadName}
    >
      <div
        className={classNames(
          "flex min-h-0 w-full flex-1 flex-col items-center justify-center overflow-hidden rounded-lg border border-dashed text-center",
          isGridLayout ? "px-2" : "px-4",
          isUserMessage
            ? isDark
              ? "border-white/20 bg-black/10"
              : "border-[rgba(15,23,42,0.24)] bg-[rgb(241,245,249)]"
            : isDark
              ? "border-white/12 bg-slate-950/70"
              : "border-[rgba(15,23,42,0.14)] bg-white/85",
        )}
      >
        <ImageIcon
          size={isGridLayout ? 20 : 24}
          className={classNames(
            "flex-shrink-0 opacity-75",
            isGridLayout ? "mb-1" : "mb-3",
            isDark ? "text-white" : "text-[rgb(71,85,105)]",
          )}
        />
        <div
          className={classNames(
            "w-full font-semibold",
            isGridLayout ? "line-clamp-2 break-words text-[11px] leading-4" : "text-xs",
            isDark ? "text-white" : "text-[rgb(30,41,59)]",
          )}
        >
          {unavailableLabel}
        </div>
        <div
          className={classNames(
            "w-full",
            isGridLayout ? "mt-0.5 truncate text-[10px] leading-4" : "mt-1 text-[11px]",
            isDark ? "text-white/72" : "text-[rgb(100,116,139)]",
          )}
        >
          {openOriginalLabel}
        </div>
      </div>
      <div className={classNames("min-w-0 px-1", isGridLayout ? "pt-1" : "pt-2")}>
        <div
          className={classNames(
            "truncate text-[11px] font-medium",
            isDark ? "text-white/88" : "text-[rgb(51,65,85)]",
          )}
        >
          {alt}
        </div>
      </div>
    </a>
  );
}
