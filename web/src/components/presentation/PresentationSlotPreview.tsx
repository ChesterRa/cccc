import { useState } from "react";
import { File, FileText, Globe, Image, Table } from "lucide-react";
import type { PresentationSlot } from "../../types";
import { getPresentationReferenceHref } from "./presentationAssets";
import { LcdIcon } from "../Icons";

/** Overview previews never mount a live browser, PDF viewer or rich document renderer. */
export function PresentationSlotPreview({
  groupId,
  slot,
}: {
  groupId: string;
  slot: PresentationSlot;
}) {
  const [imageFailed, setImageFailed] = useState(false);
  const card = slot.card;
  if (!card) return null;
  const [Icon, glyph] =
    card.card_type === "image"
      ? ([Image, "image"] as const)
      : card.card_type === "markdown"
        ? ([FileText, "file-text"] as const)
        : card.card_type === "table"
          ? ([Table, "grid-3x3"] as const)
          : card.card_type === "web_preview"
            ? ([Globe, "globe"] as const)
            : ([File, "file"] as const);
  if (card.card_type === "image" && groupId && !imageFailed) {
    return (
      <img
        src={getPresentationReferenceHref(groupId, slot, card.published_at)}
        alt=""
        loading="lazy"
        decoding="async"
        className="h-full w-full object-contain"
        onError={() => setImageFailed(true)}
      />
    );
  }
  return (
    <LcdIcon icon={Icon} glyph={glyph} className="h-5 w-5 text-[var(--color-text-secondary)]" />
  );
}
