import { useState } from "react";
import { File, FileText, Globe, Image, Table } from "lucide-react";
import type { PresentationSlot } from "../../types";
import { getPresentationReferenceHref } from "./presentationAssets";

/** Overview previews never mount a live browser, PDF viewer or rich document renderer. */
export function PresentationSlotPreview({
  groupId,
  slot,
  compact = false,
}: {
  groupId: string;
  slot: PresentationSlot;
  compact?: boolean;
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
  if (compact) return <Icon className="h-5 w-5 text-[var(--color-text-secondary)]" />;
  if (card.card_type === "markdown" && card.content.markdown) {
    const lines = card.content.markdown
      .slice(0, 1200)
      .split(/\r?\n/)
      .filter((line) => line.trim())
      .slice(0, 5);
    return (
      <div className="w-full space-y-1.5 self-start p-3 text-left text-xs leading-5 text-[var(--color-text-secondary)]">
        {lines.map((line, index) => (
          <p
            key={index}
            className={
              /^#{1,6}\s/.test(line)
                ? "truncate font-semibold text-[var(--color-text-primary)]"
                : "line-clamp-2"
            }
          >
            {line.replace(/^#{1,6}\s+/, "")}
          </p>
        ))}
      </div>
    );
  }
  if (card.card_type === "table" && card.content.table) {
    const table = card.content.table;
    return (
      <div className="w-full self-start overflow-hidden p-2" aria-hidden="true">
        <table className="w-full table-fixed text-left text-[10px]">
          <thead>
            <tr>
              {table.columns.slice(0, 4).map((cell, index) => (
                <th
                  key={index}
                  className="truncate border-b border-[var(--glass-border-subtle)] p-1.5 font-semibold"
                >
                  {cell}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {table.rows.slice(0, 3).map((row, index) => (
              <tr key={index}>
                {row.slice(0, 4).map((cell, col) => (
                  <td
                    key={col}
                    className="truncate border-b border-[var(--glass-border-subtle)] p-1.5 text-[var(--color-text-secondary)]"
                  >
                    {cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }
  let source = card.content.workspace_rel_path || card.source_label || "";
  if (card.card_type === "web_preview" && card.content.url) {
    try {
      source = new URL(card.content.url).host;
    } catch {
      /* The title remains usable without a URL label. */
    }
  }
  return (
    <div className="flex min-w-0 flex-col items-center gap-2 p-3 text-[var(--color-text-tertiary)]">
      <Icon className="h-7 w-7" />
      {source && <span className="max-w-full truncate text-xs">{source}</span>}
      {card.summary && <p className="line-clamp-2 text-xs leading-5">{card.summary}</p>}
    </div>
  );
}
