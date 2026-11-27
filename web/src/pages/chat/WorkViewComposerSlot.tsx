import type { ReactNode } from "react";
import { getChatSession, useUIStore } from "../../stores/useUIStore";

/**
 * Holds the message composer below the work area. In the terminals view the terminals
 * take the full height, so the composer is hidden; it stays mounted, keeping its draft,
 * recipients and attachments for the return to messages.
 */
export function WorkViewComposerSlot({
  groupId,
  children,
}: {
  groupId: string;
  children: ReactNode;
}) {
  const terminals = useUIStore(
    (state) => getChatSession(groupId, state.chatSessions).workView === "terminals",
  );
  return (
    <div
      className={terminals ? "hidden" : "contents"}
      hidden={terminals}
      inert={terminals || undefined}
      data-work-view-composer={terminals ? "hidden" : "visible"}
    >
      {children}
    </div>
  );
}
