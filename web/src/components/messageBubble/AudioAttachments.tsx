import {
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type SyntheticEvent,
} from "react";
import { useTranslation } from "react-i18next";
import type { MessageAttachment } from "../../types";
import { classNames } from "../../utils/classNames";
import {
  buildMessageAttachmentLinks,
  type MessageAttachmentLinks,
} from "../../utils/messageAttachmentLinks";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { DownloadIcon, PauseIcon, PlayIcon } from "../Icons";
import { formatAudioTime, pauseOtherMessageMedia } from "./messageMedia";

const ignoreRefusedPlay = () => undefined;
const SEEK_STEP_SECONDS = 5;
const SEEK_PAGE_SECONDS = 15;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function finiteDuration(value: number): number {
  return Number.isFinite(value) && value > 0 ? value : 0;
}

/**
 * Audio files in a message. Clicking a file plays or pauses it; a separate key at
 * its end downloads it. The active clip gains a compact seek bar and time readout
 * once its duration is known, while inactive clips stay compact for quick switching.
 * The message has one hidden audio element shared by its clips, so several can be
 * compared one after another. The element stays mounted without a
 * source and takes it inside the click: mobile browsers only start playback from
 * the gesture itself, not from an effect after a render.
 */
export function AudioAttachments({
  attachments,
  blobGroupId,
  attachmentKeyPrefix,
  downloadTitle,
}: {
  attachments: MessageAttachment[];
  blobGroupId: string;
  attachmentKeyPrefix: string;
  downloadTitle: (name: string) => string;
}) {
  const { t } = useTranslation("chat");
  const player = useRef<HTMLAudioElement>(null);
  const dragPointerId = useRef<number | null>(null);
  const [current, setCurrent] = useState<{ key: string; links: MessageAttachmentLinks } | null>(
    null,
  );
  const [playing, setPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  // While the user drags the seek bar, hold the preview position until pointer release
  // so rapid moves do not flood byte-range requests or snap back on intermediate timeupdate.
  const [dragTime, setDragTime] = useState<number | null>(null);
  // An error can be a dropped connection as well as an undecodable file, so a
  // failed file stays clickable and the note lasts until the next attempt.
  const [failed, setFailed] = useState("");

  const hasDuration = duration > 0;
  const shownTime = dragTime !== null ? dragTime : currentTime;
  const progress = hasDuration ? clamp(shownTime / duration, 0, 1) : 0;

  const resetPlaybackMetrics = () => {
    dragPointerId.current = null;
    setDragTime(null);
    setDuration(0);
    setCurrentTime(0);
  };

  const syncMediaTime = (audio: HTMLAudioElement) => {
    const nextDuration = finiteDuration(audio.duration);
    setDuration(nextDuration);
    if (nextDuration > 0) {
      const nextTime = Number.isFinite(audio.currentTime)
        ? clamp(audio.currentTime, 0, nextDuration)
        : 0;
      setCurrentTime(nextTime);
    } else {
      setCurrentTime(0);
    }
  };

  const seekTo = (nextTime: number) => {
    const audio = player.current;
    if (!audio || !hasDuration) return;
    const clamped = clamp(nextTime, 0, duration);
    audio.currentTime = clamped;
    setCurrentTime(clamped);
  };

  const timeFromPointer = (event: PointerEvent<HTMLDivElement>): number | null => {
    if (!hasDuration) return null;
    const rect = event.currentTarget.getBoundingClientRect();
    if (rect.width <= 0) return null;
    const ratio = clamp((event.clientX - rect.left) / rect.width, 0, 1);
    return ratio * duration;
  };

  const handleSeekPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    const next = timeFromPointer(event);
    if (next === null) return;
    event.stopPropagation();
    dragPointerId.current = event.pointerId;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDragTime(next);
  };

  const handleSeekPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (dragPointerId.current !== event.pointerId) return;
    const next = timeFromPointer(event);
    if (next !== null) setDragTime(next);
  };

  const handleSeekPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (dragPointerId.current !== event.pointerId) return;
    const next = timeFromPointer(event);
    dragPointerId.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture?.(event.pointerId);
    }
    const target = next ?? dragTime;
    setDragTime(null);
    if (target !== null) seekTo(target);
  };

  const handleSeekPointerCancel = (event: PointerEvent<HTMLDivElement>) => {
    if (dragPointerId.current !== event.pointerId) return;
    dragPointerId.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture?.(event.pointerId);
    }
    setDragTime(null);
  };

  const handleSeekKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!hasDuration) return;
    let target: number | null = null;
    switch (event.key) {
      case "ArrowLeft":
      case "ArrowDown":
        target = shownTime - SEEK_STEP_SECONDS;
        break;
      case "ArrowRight":
      case "ArrowUp":
        target = shownTime + SEEK_STEP_SECONDS;
        break;
      case "PageDown":
        target = shownTime - SEEK_PAGE_SECONDS;
        break;
      case "PageUp":
        target = shownTime + SEEK_PAGE_SECONDS;
        break;
      case "Home":
        target = 0;
        break;
      case "End":
        target = duration;
        break;
      default:
        return;
    }
    event.preventDefault();
    setDragTime(null);
    seekTo(target);
  };

  const toggle = (itemKey: string, clip: MessageAttachmentLinks) => {
    const audio = player.current;
    if (!audio) return;
    setFailed("");
    if (current?.key !== itemKey) {
      const sameSource = current?.links.previewHref === clip.previewHref;
      setCurrent({ key: itemKey, links: clip });
      if (sameSource) {
        dragPointerId.current = null;
        setDragTime(null);
        audio.currentTime = 0;
        syncMediaTime(audio);
      } else {
        audio.src = clip.previewHref;
        resetPlaybackMetrics();
        // A new source stops playback without a pause event; the play event sets this back.
        setPlaying(false);
      }
    } else if (!audio.paused) {
      audio.pause();
      return;
    }
    // A refused or interrupted start needs nothing here: an undecodable file
    // reports through the element's error event, and the player stays usable.
    void audio.play().catch(ignoreRefusedPlay);
  };

  const fail = (event: SyntheticEvent<HTMLAudioElement>) => {
    // Loading a new source clears the error, so a late event from the clip left
    // behind cannot mark the one now playing.
    if (!current || !event.currentTarget.error) return;
    setFailed(current.links.label);
    setCurrent(null);
    setPlaying(false);
    resetPlaybackMetrics();
  };

  const progressWidth = `${Math.round(progress * 1000) / 10}%`;

  return (
    <div className="flex max-w-full flex-col items-start gap-2">
      <div className="flex max-w-full flex-wrap items-start gap-2">
        {attachments.map((attachment, index) => {
          const itemKey = `audio:${attachmentKeyPrefix}:${index}`;
          const links = buildMessageAttachmentLinks({
            groupId: blobGroupId,
            path: attachment.path,
            title: attachment.title,
            localPreviewUrl: attachment.local_preview_url,
            fallbackLabel: "audio",
          });
          const active = current?.key === itemKey;
          const showSeekBar = active && hasDuration;
          const Icon = active && playing ? PauseIcon : PlayIcon;
          return (
            <div
              key={itemKey}
              className={classNames(
                "relative inline-flex max-w-full flex-col border border-[var(--glass-border-subtle)] text-xs text-[var(--color-text-secondary)]",
                showSeekBar ? "min-w-[min(100%,220px)] rounded-2xl" : "rounded-full",
                active && "bg-[var(--glass-tab-bg)]",
              )}
              data-message-audio
            >
              {active && !showSeekBar && (
                <span
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit]"
                >
                  <span
                    data-message-audio-progress
                    className="absolute bottom-0 left-0 h-0.5 bg-[var(--color-text-primary)] transition-[width] duration-200"
                    style={{ width: progressWidth }}
                  />
                </span>
              )}
              <div className="flex min-w-0 max-w-full items-stretch">
                <button
                  type="button"
                  onClick={() => toggle(itemKey, links)}
                  aria-label={t(active && playing ? "pauseAudio" : "playAudio", {
                    name: links.label,
                  })}
                  data-message-audio-toggle
                  className={classNames(
                    "flex min-h-10 min-w-0 flex-1 items-center gap-1.5 py-1.5 pl-2.5 pr-2 transition-colors hover:bg-[var(--glass-tab-bg)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-border-focus)] md:min-h-8",
                    showSeekBar ? "rounded-tl-2xl" : "rounded-l-full",
                  )}
                >
                  <Icon size={14} className="flex-shrink-0" />
                  <span
                    className="truncate font-medium"
                    style={{ color: "var(--color-text-primary)" }}
                  >
                    {links.label}
                  </span>
                </button>
                <AuthenticatedDownloadLink
                  href={links.downloadHref}
                  download={links.downloadName}
                  title={downloadTitle(links.label)}
                  aria-label={downloadTitle(links.label)}
                  className={classNames(
                    "flex min-h-10 min-w-10 flex-shrink-0 items-center justify-center border-l border-[var(--glass-border-subtle)] pr-1 transition-colors hover:bg-[var(--glass-tab-bg)] md:min-h-8 md:min-w-8",
                    showSeekBar ? "rounded-tr-2xl" : "rounded-r-full",
                  )}
                >
                  <DownloadIcon size={13} />
                </AuthenticatedDownloadLink>
              </div>
              {showSeekBar && (
                <div className="flex min-w-0 items-center gap-2 border-t border-[var(--glass-border-subtle)] px-2.5 py-1.5">
                  <div
                    role="slider"
                    tabIndex={0}
                    aria-label={t("seekAudio", { name: links.label })}
                    aria-valuemin={0}
                    aria-valuemax={Math.round(duration)}
                    aria-valuenow={Math.round(shownTime)}
                    aria-valuetext={`${formatAudioTime(shownTime)} / ${formatAudioTime(duration)}`}
                    data-message-audio-seek
                    onPointerDown={handleSeekPointerDown}
                    onPointerMove={handleSeekPointerMove}
                    onPointerUp={handleSeekPointerUp}
                    onPointerCancel={handleSeekPointerCancel}
                    onKeyDown={handleSeekKeyDown}
                    className="group relative flex min-h-6 min-w-[72px] flex-1 cursor-pointer touch-none select-none items-center rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]"
                  >
                    <span
                      aria-hidden="true"
                      className="relative h-1.5 w-full overflow-hidden rounded-full bg-[var(--glass-border-subtle)]"
                    >
                      <span
                        data-message-audio-progress
                        className={classNames(
                          "absolute inset-y-0 left-0 rounded-full bg-[var(--color-text-primary)]",
                          dragTime === null && "transition-[width] duration-150",
                        )}
                        style={{ width: progressWidth }}
                      />
                    </span>
                    <span
                      aria-hidden="true"
                      className="pointer-events-none absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[var(--color-text-primary)] shadow-sm transition-transform group-hover:scale-110 group-focus-visible:scale-110"
                      style={{ left: progressWidth }}
                    />
                  </div>
                  <span
                    data-message-audio-time
                    className="flex-shrink-0 tabular-nums text-[11px] text-[var(--color-text-muted)]"
                  >
                    {formatAudioTime(shownTime)} / {formatAudioTime(duration)}
                  </span>
                </div>
              )}
            </div>
          );
        })}
      </div>
      {failed && (
        <p className="text-xs text-[var(--color-text-muted)]">
          {t("audioPlaybackUnavailable", { name: failed })}
        </p>
      )}
      {/* Playback is driven from the chips alone: the element shows no controls, and
          the playing chip carries the clip's name, seek bar and duration. */}
      <audio
        ref={player}
        preload="none"
        hidden
        data-message-media
        aria-label={current?.links.label}
        onPlay={(event) => {
          setPlaying(true);
          pauseOtherMessageMedia(event);
        }}
        onPause={() => setPlaying(false)}
        onLoadedMetadata={(event) => syncMediaTime(event.currentTarget)}
        onDurationChange={(event) => syncMediaTime(event.currentTarget)}
        onTimeUpdate={(event) => syncMediaTime(event.currentTarget)}
        onSeeked={(event) => syncMediaTime(event.currentTarget)}
        onEnded={(event) => {
          const total = finiteDuration(event.currentTarget.duration);
          if (total > 0) {
            setDuration(total);
            setCurrentTime(total);
          }
        }}
        onError={fail}
      />
    </div>
  );
}
