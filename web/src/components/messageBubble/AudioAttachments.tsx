import { useRef, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import type { MessageAttachment } from "../../types";
import { classNames } from "../../utils/classNames";
import {
  buildMessageAttachmentLinks,
  type MessageAttachmentLinks,
} from "../../utils/messageAttachmentLinks";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { PauseIcon, PlayIcon } from "../Icons";
import { pauseOtherMessageMedia } from "./messageMedia";

const ignoreRefusedPlay = () => undefined;

/**
 * Audio files in a message. Each keeps its download chip and gains a play button.
 * The message has one player, shown once a file is chosen, so several clips can be
 * compared without stacking a player per file. The element stays mounted without a
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
                "inline-flex max-w-full items-stretch rounded-full border border-[var(--glass-border-subtle)] text-xs text-[var(--color-text-secondary)]",
                active && "bg-[var(--glass-tab-bg)]",
              )}
              data-message-audio
            >
              <button
                type="button"
                onClick={() => toggle(links)}
                aria-label={t(active && playing ? "pauseAudio" : "playAudio", {
                  name: links.label,
                })}
                className="flex min-h-10 min-w-10 flex-shrink-0 items-center justify-center rounded-l-full pl-1 transition-colors hover:bg-[var(--glass-tab-bg)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-border-focus)] md:min-h-8 md:min-w-8"
              >
                <Icon size={14} />
              </button>
              <AuthenticatedDownloadLink
                href={links.downloadHref}
                download={links.downloadName}
                title={downloadTitle(links.label)}
                className="inline-flex min-w-0 items-center rounded-r-full py-1.5 pl-1 pr-2.5 transition-colors hover:bg-[var(--glass-tab-bg)]"
              >
                <span
                  className="truncate font-medium"
                  style={{ color: "var(--color-text-primary)" }}
                >
                  {links.label}
                </span>
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
      <div hidden={!current} className="w-[min(30rem,82vw)] max-w-full" data-message-audio-player>
        <p className="truncate text-xs font-medium text-[var(--color-text-primary)]">
          {current?.label}
        </p>
        <audio
          ref={player}
          controls
          preload="none"
          data-message-media
          aria-label={current?.label}
          onPlay={(event) => {
            setPlaying(true);
            pauseOtherMessageMedia(event);
          }}
          onPause={() => setPlaying(false)}
          onError={fail}
          className="mt-1 block w-full"
        />
      </div>
    </div>
  );
}
