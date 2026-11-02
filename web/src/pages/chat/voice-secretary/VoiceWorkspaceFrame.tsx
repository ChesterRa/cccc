import { useCallback, useRef, type ReactNode } from "react";
import { VoiceBeam } from "voice-glow";

const LEVEL_SEGMENTS = 16;

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
  const levelBarRef = useRef<HTMLSpanElement>(null);
  // Reuse VoiceBeam's smoothed level callback; it already owns the animation loop.
  // The LCD meter lights whole segments, 16 of them, like a level meter on a device.
  const updateLevelBar = useCallback((value: number) => {
    if (!levelBarRef.current) return;
    const bounded = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
    const lit = Math.round(bounded * LEVEL_SEGMENTS);
    levelBarRef.current.style.clipPath = `inset(0 ${100 - (lit * 100) / LEVEL_SEGMENTS}% 0 0)`;
  }, []);
  return (
    <VoiceBeam
      type="mobile"
      active={recording || analyzing}
      level={level}
      processing={analyzing}
      theme={isDark ? "dark" : "light"}
      strength={0}
      coreLight={0}
      onLevel={recording ? updateLevelBar : undefined}
      borderRadius={0}
      data-voice-workspace-glow
      className="flex min-h-0 min-w-0 flex-1 flex-col"
    >
      <section
        data-voice-document-panel
        className="flex min-h-0 flex-1 flex-col bg-[var(--color-bg-primary)] px-4 pb-4 pt-3 sm:px-6"
      >
        {children}
      </section>
      {recording || analyzing ? (
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
