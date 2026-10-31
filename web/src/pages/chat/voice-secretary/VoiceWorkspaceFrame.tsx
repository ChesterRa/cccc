import { useCallback, useRef, type ReactNode } from "react";
import { VoiceBeam } from "voice-glow";
import { useLcdStyle } from "../../../utils/uiStyle";

export function VoiceWorkspaceFrame({
  recording,
  processing,
  level,
  isDark,
  children,
}: {
  recording: boolean;
  processing: boolean;
  level: () => number;
  isDark: boolean;
  children: ReactNode;
}) {
  const analyzing = !recording && processing;
  const lcd = useLcdStyle();
  const levelBarRef = useRef<HTMLSpanElement>(null);
  // Reuse VoiceBeam's smoothed level callback; it already owns the animation loop.
  const updateLevelBar = useCallback((value: number) => {
    if (!levelBarRef.current) return;
    const bounded = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
    levelBarRef.current.style.transform = `scaleX(${bounded})`;
  }, []);
  return (
    <VoiceBeam
      type="mobile"
      active={recording || analyzing}
      level={level}
      processing={analyzing}
      theme={isDark ? "dark" : "light"}
      strength={lcd ? 0 : undefined}
      coreLight={lcd ? 0 : undefined}
      onLevel={lcd && recording ? updateLevelBar : undefined}
      borderRadius={lcd ? 0 : undefined}
      data-voice-workspace-glow
      className="flex min-h-0 min-w-0 flex-1 flex-col"
    >
      <section
        data-voice-document-panel
        className="flex min-h-0 flex-1 flex-col bg-[var(--color-bg-primary)] px-4 pb-4 pt-3 sm:px-6"
      >
        {children}
      </section>
      {lcd && (recording || analyzing) ? (
        <span
          data-voice-workspace-meter={recording ? "recording" : "processing"}
          aria-hidden="true"
        >
          {recording ? <span ref={levelBarRef} /> : null}
        </span>
      ) : null}
    </VoiceBeam>
  );
}
