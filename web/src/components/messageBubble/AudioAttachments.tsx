import { useRef, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import type { MessageAttachment } from "../../types";
import { classNames } from "../../utils/classNames";
import {
  buildMessageAttachmentLinks,
  type MessageAttachmentLinks,
} from "../../utils/messageAttachmentLinks";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { DownloadIcon, PauseIcon, PlayIcon } from "../Icons";
import { pauseOtherMessageMedia } from "./messageMedia";

const ignoreRefusedPlay = () => undefined;

/**
 * Audio files in a message. Clicking a file plays or pauses it; a separate key at
 * its end downloads it. There is no separate player: the playing chip shows progress.
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
  const [current, setCurrent] = useState<MessageAttachmentLinks | null>(null);
  const [playing, setPlaying] = useState(false);
  // Share of the current clip already played, shown along the bottom of its chip.
  const [progress, setProgress] = useState(0);
  // An error can be a dropped connection as well as an undecodable file, so a
  // failed file stays clickable and the note lasts until the next attempt.
  const [failed, setFailed] = useState("");

  const toggle = (clip: MessageAttachmentLinks) => {
    const audio = player.current;
    if (!audio) return;
    setFailed("");
    if (current?.previewHref !== clip.previewHref) {
      audio.src = clip.previewHref;
      setCurrent(clip);
      setProgress(0);
      // A new source stops playback without a pause event; the play event sets this back.
      setPlaying(false);
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
    setFailed(current.label);
    setCurrent(null);
    setPlaying(false);
  };

  return (
    <div className="flex max-w-full flex-col items-start gap-2">
      <div className="flex max-w-full flex-wrap items-start gap-2">
        {attachments.map((attachment, index) => {
          const links = buildMessageAttachmentLinks({
            groupId: blobGroupId,
            path: attachment.path,
            title: attachment.title,
            localPreviewUrl: attachment.local_preview_url,
            fallbackLabel: "audio",
          });
          const active = current?.previewHref === links.previewHref;
          const Icon = active && playing ? PauseIcon : PlayIcon;
          return (
            <div
              key={`audio:${attachmentKeyPrefix}:${index}`}
              className={classNames(
                "relative inline-flex max-w-full items-stretch overflow-hidden rounded-full border border-[var(--glass-border-subtle)] text-xs text-[var(--color-text-secondary)]",
                active && "bg-[var(--glass-tab-bg)]",
              )}
              data-message-audio
            >
              {active && (
                <span
                  aria-hidden="true"
                  data-message-audio-progress
                  className="pointer-events-none absolute bottom-0 left-0 h-0.5 bg-[var(--color-text-primary)] transition-[width] duration-200"
                  style={{ width: `${Math.round(progress * 1000) / 10}%` }}
                />
              )}
              <button
                type="button"
                onClick={() => toggle(links)}
                aria-label={t(active && playing ? "pauseAudio" : "playAudio", {
                  name: links.label,
                })}
                data-message-audio-toggle
                className="flex min-h-10 min-w-0 items-center gap-1.5 rounded-l-full py-1.5 pl-2.5 pr-2 transition-colors hover:bg-[var(--glass-tab-bg)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-border-focus)] md:min-h-8"
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
                className="flex min-h-10 min-w-10 flex-shrink-0 items-center justify-center rounded-r-full border-l border-[var(--glass-border-subtle)] pr-1 transition-colors hover:bg-[var(--glass-tab-bg)] md:min-h-8 md:min-w-8"
              >
                <DownloadIcon size={13} />
              </AuthenticatedDownloadLink>
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
          the playing chip carries the clip's name and progress. */}
      <audio
        ref={player}
        preload="none"
        hidden
        data-message-media
        aria-label={current?.label}
        onPlay={(event) => {
          setPlaying(true);
          pauseOtherMessageMedia(event);
        }}
        onPause={() => setPlaying(false)}
        onTimeUpdate={(event) => {
          const { currentTime, duration } = event.currentTarget;
          setProgress(duration > 0 && Number.isFinite(duration) ? currentTime / duration : 0);
        }}
        onError={fail}
      />
    </div>
  );
}
