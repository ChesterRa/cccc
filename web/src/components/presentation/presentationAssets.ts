import type { PresentationSlot } from "../../types";
import { getPresentationAssetUrl, refreshAuthTokenInUrl } from "../../services/api";

export function getPresentationReferenceHref(
  groupId: string,
  slot: PresentationSlot | null,
  cacheBust?: string | number,
): string {
  const card = slot?.card;
  if (!card) return "";
  const url = String(card.content.url || "").trim();
  return url
    ? refreshAuthTokenInUrl(url)
    : getPresentationAssetUrl(groupId, slot.slot_id, cacheBust);
}
