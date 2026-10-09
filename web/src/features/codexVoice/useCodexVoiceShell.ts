import { useState } from "react";
import { useCodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { useCodexVoiceAnalystSettings } from "./useCodexVoiceAnalystSettings";

export function useCodexVoiceShell(enabled: boolean) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [analystSettingsActive, setAnalystSettingsActive] = useState(false);
  const controller = useCodexVoiceSessionController(enabled);
  const analystSettings = useCodexVoiceAnalystSettings(
    enabled && analystSettingsActive,
    controller,
  );
  return {
    controller,
    analystSettings,
    setAnalystSettingsActive,
    detailsOpen,
    start: () => {
      if (!controller.isEngaged) void controller.start();
    },
    openDetails: () => setDetailsOpen(true),
    closeDetails: () => setDetailsOpen(false),
  };
}
export type CodexVoiceShellState = ReturnType<typeof useCodexVoiceShell>;
