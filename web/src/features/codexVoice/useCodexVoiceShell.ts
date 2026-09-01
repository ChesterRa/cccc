import { useState } from "react";
import { useCodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function useCodexVoiceShell(enabled: boolean, selectedGroupId: string) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const controller = useCodexVoiceSessionController(enabled);

  const start = () => {
    const launchGroupId = controller.analyst?.group_id || selectedGroupId;
    if (!launchGroupId || controller.isEngaged) return;
    setDetailsOpen(true);
    void controller.start(launchGroupId);
  };
  return {
    controller,
    detailsOpen,
    start,
    openDetails: () => setDetailsOpen(true),
    closeDetails: () => setDetailsOpen(false),
  };
}

export type CodexVoiceShellState = ReturnType<typeof useCodexVoiceShell>;
