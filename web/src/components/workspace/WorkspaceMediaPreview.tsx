import { useEffect, useRef, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";

/** The browser owns playback, seeking, volume and fullscreen. No eager download or autoplay. */
export function WorkspaceMediaPreview({
  src,
  name,
  audio = false,
  failedText,
  onPlay,
  visible = true,
}: {
  src: string;
  name: string;
  audio?: boolean;
  /** Off screen, such as behind another tab: playback stops, as it would not be heard on purpose. */
  visible?: boolean;
  /** Replaces the advice to download, for a preview that has no download. */
  failedText?: string;
  /** Given in a message, where one piece of media plays at a time. */
  onPlay?: (event: SyntheticEvent<HTMLMediaElement>) => void;
}) {
  const { t } = useTranslation("chat");
  const [failed, setFailed] = useState(false);
  const media = useRef<HTMLMediaElement>(null);
  useEffect(() => {
    if (!visible) media.current?.pause();
  }, [visible]);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-3 overflow-auto p-3">
      {failed ? (
        <p role="alert" className="max-w-md text-center text-sm text-[var(--color-text-secondary)]">
          {failedText ?? t(audio ? "workspaceAudioUnavailable" : "workspaceVideoUnavailable")}
        </p>
      ) : audio ? (
        <audio
          ref={(node) => {
            media.current = node;
          }}
          src={src}
          aria-label={name}
          controls
          preload="metadata"
          data-message-media={onPlay ? true : undefined}
          onPlay={onPlay}
          onError={() => setFailed(true)}
          className="w-full max-w-lg"
        />
      ) : (
        <video
          ref={(node) => {
            media.current = node;
          }}
          className="min-h-0 max-h-full w-full bg-black"
          src={src}
          aria-label={name}
          controls
          playsInline
          preload="metadata"
          data-message-media={onPlay ? true : undefined}
          onPlay={onPlay}
          onError={() => setFailed(true)}
        />
      )}
    </div>
  );
}
