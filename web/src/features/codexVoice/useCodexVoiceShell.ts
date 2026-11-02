import { useMemo, useState } from "react";
import { useGroupStore, useUIStore } from "../../stores";
import type { ChetGroupContext } from "../../services/api/chet";
import { useChetConversation } from "../chet/useChetConversation";
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
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  const groupDoc = useGroupStore((state) => state.groupDoc);
  const groups = useGroupStore((state) => state.groups);
  // The open group, as a label Chet carries with a message; it grants nothing.
  const currentContext = useMemo<ChetGroupContext | null>(() => {
    const groupId = String(selectedGroupId || "").trim();
    if (!groupId) return null;
    const doc = groupDoc?.group_id === groupId ? groupDoc : null;
    const title =
      doc?.title?.trim() ||
      groups.find((group) => group.group_id === groupId)?.title?.trim() ||
      groupId;
    return { group_id: groupId, group_title: title, scope_key: doc?.active_scope_key || null };
  }, [groupDoc, groups, selectedGroupId]);
  // Only a confirmed administrator's page reads Chet's history in the background; any other
  // page would be refused each time.
  const isWebAdmin = useUIStore((state) => state.isWebAdmin);
  const chet = useChetConversation({
    enabled,
    open: detailsOpen,
    followClosed: isWebAdmin === true,
    currentContext,
    // The first send can start the shared Analyst; read it so its status shows.
    onRegistered: () => void controller.refresh(false),
  });
  return {
    controller,
    analystSettings,
    setAnalystSettingsActive,
    chet,
    detailsOpen,
    start: () => {
      if (!controller.isEngaged) void controller.start();
    },
    openDetails: () => setDetailsOpen(true),
    closeDetails: () => setDetailsOpen(false),
  };
}
export type CodexVoiceShellState = ReturnType<typeof useCodexVoiceShell>;
