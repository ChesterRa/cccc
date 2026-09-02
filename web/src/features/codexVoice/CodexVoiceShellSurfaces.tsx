import { CodexVoiceDock } from "../../components/layout/CodexVoiceDock";
import { CodexVoiceAnalystModal } from "../../components/modals/CodexVoiceAnalystModal";
import type { CodexVoiceShellState } from "./useCodexVoiceShell";

type SharedProps = { voice: CodexVoiceShellState };

export function CodexVoiceSidebarDock(props: SharedProps & { collapsed: boolean }) {
  const { voice, collapsed } = props;
  return (
    <CodexVoiceDock
      controller={voice.controller}
      collapsed={collapsed}
      onOpen={voice.openDetails}
      onStart={voice.start}
    />
  );
}

export function CodexVoiceMobileDock(props: SharedProps) {
  const { voice } = props;
  if (!voice.controller.isEngaged) return null;
  return (
    <div className="mt-14 flex-none md:hidden">
      <CodexVoiceDock
        variant="mobile"
        controller={voice.controller}
        onOpen={voice.openDetails}
        onStart={voice.start}
      />
    </div>
  );
}

export function CodexVoiceOverlays(
  props: SharedProps & { isDark: boolean; isSmallScreen: boolean },
) {
  const { voice, isDark, isSmallScreen } = props;
  return (
    <>
      <audio ref={voice.controller.audioRef} autoPlay playsInline className="hidden" />
      <CodexVoiceAnalystModal
        isOpen={voice.detailsOpen}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        controller={voice.controller}
        onClose={voice.closeDetails}
      />
    </>
  );
}
