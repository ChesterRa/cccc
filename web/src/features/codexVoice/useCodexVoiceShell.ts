import { useState } from "react";
import { useCodexVoiceSessionController } from "./useCodexVoiceSessionController";

export function useCodexVoiceShell(enabled: boolean) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const controller = useCodexVoiceSessionController(enabled);

  const start = () => {
    if (controller.isEngaged) return;
    void controller.start();
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
