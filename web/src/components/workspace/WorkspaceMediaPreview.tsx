import { useState } from "react";
import { useTranslation } from "react-i18next";

/** The browser owns playback, seeking, volume and fullscreen. No eager download or autoplay. */
export function WorkspaceMediaPreview({
  src,
  name,
  audio = false,
}: {
  src: string;
  name: string;
  audio?: boolean;
}) {
  const { t } = useTranslation("chat");
  const [failed, setFailed] = useState(false);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-3 overflow-auto p-3">
      {failed ? (
        <p role="alert" className="max-w-md text-center text-sm text-[var(--color-text-secondary)]">
          {t(audio ? "workspaceAudioUnavailable" : "workspaceVideoUnavailable")}
        </p>
      ) : audio ? (
        <audio
          src={src}
          aria-label={name}
          controls
          preload="metadata"
          onError={() => setFailed(true)}
          className="w-full max-w-lg"
        />
      ) : (
        <video
          className="min-h-0 max-h-full w-full bg-black"
          src={src}
          aria-label={name}
          controls
          playsInline
          preload="metadata"
          onError={() => setFailed(true)}
        />
      )}
    </div>
  );
}
