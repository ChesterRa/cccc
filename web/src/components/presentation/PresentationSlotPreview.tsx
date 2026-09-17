import { useState } from "react";
import { File, FileText, Globe, Image, Table } from "lucide-react";
import type { PresentationSlot } from "../../types";
import { getPresentationReferenceHref } from "./presentationAssets";

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
  const Icon =
    card.card_type === "image"
      ? Image
      : card.card_type === "markdown"
        ? FileText
        : card.card_type === "table"
          ? Table
          : card.card_type === "web_preview"
            ? Globe
            : File;
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
  return <Icon className="h-5 w-5 text-[var(--color-text-secondary)]" />;
}
