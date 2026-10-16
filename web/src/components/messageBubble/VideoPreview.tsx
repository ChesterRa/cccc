import { useEffect, useRef, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { DownloadIcon, FileIcon, PlayIcon } from "../Icons";
import { MediaLightbox } from "./MediaLightbox";

/**
 * A video attachment shown as its first frame. The frame loads only once the
 * message is on screen; playback happens in a dialog opened by a click, where
 * the player fetches in byte ranges. Starting one video pauses the others, and
 * a format the browser cannot decode falls back to the download chip.
 */
export function VideoPreview({
  href,
  downloadHref,
  downloadName,
  label,
  downloadTitle,
}: {
  href: string;
  downloadHref: string;
  downloadName: string;
  label: string;
  downloadTitle: string;
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  // Failure belongs to one URL: a pending upload's local preview is replaced
  // by the stored blob, which must get its own chance to play.
  const [failedHref, setFailedHref] = useState("");
  const failed = failedHref === href;
  const frame = useRef<HTMLButtonElement>(null);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const target = frame.current;
    if (visible || !target) return;
    if (typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        setVisible(true);
        observer.disconnect();
      }
    });
    observer.observe(target);
    return () => observer.disconnect();
  }, [visible]);
  const fail = () => {
    setOpen(false);
    setFailedHref(href);
  };

  if (failed) {
    return (
      <div className="flex min-w-0 max-w-full flex-col items-start gap-1" data-video-fallback>
        <AuthenticatedDownloadLink
          href={downloadHref}
          download={downloadName}
          title={downloadTitle}
          aria-label={downloadTitle}
          className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-full border border-[var(--glass-border-subtle)] px-2.5 py-1.5 text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg)]"
        >
          <FileIcon size={13} className="flex-shrink-0 opacity-60" />
          <span className="truncate font-medium">{label}</span>
        </AuthenticatedDownloadLink>
        <p className="px-2 text-xs text-[var(--color-text-muted)]">
          {t("videoPlaybackUnavailable")}
        </p>
      </div>
    );
  }
  return (
    <figure className="w-full min-w-0 max-w-[min(30rem,82vw)] overflow-hidden rounded-xl border border-[var(--glass-border-subtle)]">
      <button
        ref={frame}
        type="button"
        onClick={() => setOpen(true)}
        aria-label={t("playVideo", { name: label })}
        className="group relative block w-full bg-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-border-focus)]"
      >
        {/* The first frame only: muted, no controls, never played inline. */}
        <video
          src={visible ? `${href}#t=0.1` : undefined}
          preload={visible ? "metadata" : "none"}
          muted
          playsInline
          tabIndex={-1}
          aria-hidden="true"
          className="pointer-events-none block max-h-[22rem] min-h-36 w-full object-contain"
          onError={fail}
        />
        <span className="absolute inset-0 flex items-center justify-center bg-black/10 transition-colors group-hover:bg-black/25">
          <span className="flex h-12 w-12 items-center justify-center rounded-full bg-white/90 text-black shadow-md transition-transform group-hover:scale-105">
            <PlayIcon size={22} className="translate-x-px" />
          </span>
        </span>
      </button>
      <figcaption className="flex min-w-0 items-center justify-between gap-2 py-1 pl-3 pr-1">
        <span className="truncate text-xs font-medium text-[var(--color-text-primary)]">
          {label}
        </span>
        <AuthenticatedDownloadLink
          href={downloadHref}
          download={downloadName}
          title={downloadTitle}
          aria-label={downloadTitle}
          className="inline-flex flex-shrink-0 items-center rounded-full px-2 py-1 text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg)]"
        >
          <DownloadIcon size={13} />
        </AuthenticatedDownloadLink>
      </figcaption>
      <MediaLightbox
        open={open}
        onClose={() => setOpen(false)}
        dialogLabel={t("videoPreviewDialog")}
        title={label}
        downloadHref={downloadHref}
        downloadName={downloadName}
      >
        <video
          src={href}
          controls
          autoPlay
          playsInline
          preload="auto"
          data-message-video
          onPlay={pauseOtherVideos}
          aria-label={label}
          className="h-full w-full bg-black object-contain"
          onError={fail}
        />
      </MediaLightbox>
    </figure>
  );
}

/** One message video plays at a time. */
function pauseOtherVideos(event: SyntheticEvent<HTMLVideoElement>) {
  const current = event.currentTarget;
  for (const video of current.ownerDocument.querySelectorAll<HTMLVideoElement>(
    "video[data-message-video]",
  )) {
    if (video !== current && !video.paused) video.pause();
  }
}
