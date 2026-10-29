import { useCallback, useRef, type MutableRefObject } from "react";
import { pendingReplyView } from "../../features/pendingReplies/model";
import type { LedgerEvent } from "../../types";

/** A newly followed request can be taller than the message viewport. */
export function usePendingReplyFollow({
  parentRef,
  readOnly,
  topInset,
  cancelBottomScroll,
  detach,
}: {
  parentRef: MutableRefObject<HTMLDivElement | null>;
  readOnly?: boolean;
  topInset: number;
  cancelBottomScroll: () => void;
  detach: () => void;
}) {
  const candidate = useRef("");
  const cancel = useCallback(() => {
    candidate.current = "";
  }, []);
  const consider = useCallback(
    (message: LedgerEvent) => {
      const view = readOnly ? null : pendingReplyView(message);
      candidate.current =
        message.id && (view?.state === "pending" || view?.state === "unknown") ? message.id : "";
    },
    [readOnly],
  );

  /** Return true when the request's top replaced the usual bottom target. */
  const revealIfNeeded = useCallback(() => {
    const element = parentRef.current;
    const id = candidate.current;
    if (!element || !id) return false;
    const row = element.querySelector<HTMLElement>(
      `[data-message-row="true"][data-message-id="${CSS.escape(id)}"]`,
    );
    // Status may still be loading. Until the real form appears, ordinary
    // bottom follow continues; user interaction or a later tail cancels it.
    if (!row?.querySelector("form[data-pending-reply]")) return false;
    const bottom = Math.max(0, element.scrollHeight - element.clientHeight);
    const target = Math.max(
      0,
      element.scrollTop +
        row.getBoundingClientRect().top -
        element.getBoundingClientRect().top -
        topInset -
        8,
    );
    // Markdown or diagrams can finish rendering after the initial measurement.
    // Keep watching while the user is following; explicit interaction cancels
    // the candidate, and a reveal consumes it so it cannot pull the view twice.
    if (target >= bottom - 1) return false;
    candidate.current = "";
    cancelBottomScroll();
    detach();
    element.scrollTo({ top: target, behavior: "auto" });
    return true;
  }, [cancelBottomScroll, detach, parentRef, topInset]);

  return { cancel, consider, revealIfNeeded };
}
