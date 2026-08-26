import type { GroupPresentation, PresentationSlot } from "../../types";
import { ensurePresentation } from "../../utils/presentation";

export function shouldShowMobilePresentationTrigger({
  isSmallScreen,
  hasChatWindow,
  groupId,
}: {
  isSmallScreen: boolean;
  hasChatWindow: boolean;
  groupId: string;
}): boolean {
  return isSmallScreen && !hasChatWindow && !!String(groupId || "").trim();
}

export function resolveMobilePresentationHighlight(
  presentation: GroupPresentation | null,
): PresentationSlot | null {
  const normalized = ensurePresentation(presentation);
  const highlightedId = String(normalized.highlight_slot_id || "").trim();
  return (
    normalized.slots.find((slot) => slot.slot_id === highlightedId && !!slot.card) ||
    normalized.slots.find((slot) => !!slot.card) ||
    null
  );
}
