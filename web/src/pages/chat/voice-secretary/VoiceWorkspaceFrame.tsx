import type { ReactNode } from "react";
import { VoiceBeam } from "voice-glow";

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
  return (
    <VoiceBeam
      type="mobile"
      active={recording || analyzing}
      level={level}
      processing={analyzing}
      theme={isDark ? "dark" : "light"}
      data-voice-workspace-glow
      className="flex min-h-0 min-w-0 flex-1 flex-col"
    >
      <section
        data-voice-document-panel
        className="flex min-h-0 flex-1 flex-col bg-[var(--color-bg-primary)] px-4 pb-4 pt-3 sm:px-6"
      >
        {children}
      </section>
    </VoiceBeam>
  );
}
