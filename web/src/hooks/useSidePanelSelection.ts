import { useCallback } from "react";
import { getChatSession, useUIStore } from "../stores/useUIStore";
import { useModalStore } from "../stores/useModalStore";

export type SidePanelSurface = "files" | "presentation";

/**
 * One right-hand column, two surfaces.
 *
 * The Group work controls select the column; keep switching and viewer cleanup together.
 */
export function useSidePanelSelection(groupId: string) {
  const filesPanelOpen = useUIStore((state) =>
    groupId ? getChatSession(groupId, state.chatSessions).filesPanelOpen : false,
  );
  const presentationDockOpen = useUIStore((state) =>
    groupId ? getChatSession(groupId, state.chatSessions).presentationDockOpen : false,
  );
  const setChatFilesPanelOpen = useUIStore((state) => state.setChatFilesPanelOpen);
  const setChatPresentationDockOpen = useUIStore((state) => state.setChatPresentationDockOpen);
  const presentationViewer = useModalStore((state) => state.presentationViewer);
  const setPresentationViewer = useModalStore((state) => state.setPresentationViewer);

  const activeSidePanel: SidePanelSurface | null = !groupId
    ? null
    : filesPanelOpen
      ? "files"
      : presentationDockOpen
        ? "presentation"
        : null;

  /** Selecting the active surface collapses the column; selecting the other one swaps it. */
  const selectSidePanel = useCallback(
    (surface: SidePanelSurface) => {
      if (!groupId) return;
      const next = activeSidePanel === surface ? null : surface;
      setChatFilesPanelOpen(groupId, next === "files");
      setChatPresentationDockOpen(groupId, next === "presentation");
      // A slot viewer belongs to the presentation surface; anything else releases it.
      if (
        next !== "presentation" &&
        presentationViewer?.groupId === groupId &&
        presentationViewer.surface === "split"
      ) {
        setPresentationViewer(null);
      }
    },
    [
      activeSidePanel,
      groupId,
      presentationViewer,
      setChatFilesPanelOpen,
      setChatPresentationDockOpen,
      setPresentationViewer,
    ],
  );

  return { activeSidePanel, selectSidePanel };
}
