import type {
  ChatMessageData,
  LedgerEvent,
  PresentationCard,
  PresentationMessageRef,
  PresentationRefSnapshot,
  PresentationRefStatus,
  PresentationSlot,
} from "../types";

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function trimString(value: unknown): string {
  return typeof value === "string" ? value.trim() : value == null ? "" : String(value).trim();
}

function slotIndexFromSlotId(slotId: string): number {
  const raw = trimString(slotId);
  if (!raw) return 0;
  const match = raw.match(/(\d+)$/);
  return match ? Number(match[1] || 0) || 0 : 0;
}

function slotLabelFromSlotId(slotId: string): string {
  const index = slotIndexFromSlotId(slotId);
  return index > 0 ? `P${index}` : "Presentation";
}

function defaultLocatorLabel(cardType: string): string {
  switch (trimString(cardType)) {
    case "markdown":
      return "Markdown";
    case "table":
      return "Table";
    case "image":
      return "Image";
    case "pdf":
      return "PDF";
    case "web_preview":
      return "Web";
    case "file":
      return "File";
    default:
      return "Presentation";
  }
}

export function isPresentationMessageRef(value: unknown): value is PresentationMessageRef {
  const record = asRecord(value);
  if (!record) return false;
  return trimString(record.kind) === "presentation_ref" && !!trimString(record.slot_id);
}

export function getPresentationMessageRefs(value: unknown): PresentationMessageRef[] {
  if (!Array.isArray(value)) return [];
  return value.filter(isPresentationMessageRef);
}

export function getPresentationRefStatus(
  ref: PresentationMessageRef,
  message?: ChatMessageData | null,
  event?: LedgerEvent | null,
): PresentationRefStatus {
  const raw = trimString(ref.status);
  const rawStatus =
    raw === "needs_user" || raw === "resolved" || raw === "open"
      ? (raw as PresentationRefStatus)
      : null;
  const needsReply = message?.message_mode === "request_reply";
  const obligationStatus = event?._obligation_status;
  const userObligation =
    obligationStatus &&
    typeof obligationStatus === "object" &&
    Object.prototype.hasOwnProperty.call(obligationStatus, "user") &&
    obligationStatus.user &&
    typeof obligationStatus.user === "object"
      ? obligationStatus.user
      : null;

  if (userObligation) {
    if (needsReply || !!userObligation.reply_requested) {
      return userObligation.replied ? "resolved" : "needs_user";
    }
  }

  if (
    obligationStatus &&
    typeof obligationStatus === "object" &&
    Object.keys(obligationStatus).length > 0 &&
    !Object.prototype.hasOwnProperty.call(obligationStatus, "user") &&
    needsReply
  ) {
    return rawStatus || "open";
  }

  if (rawStatus) return rawStatus;
  if (needsReply) {
    return "needs_user";
  }
  return "open";
}

export function getPresentationRefSlotId(ref: PresentationMessageRef): string {
  return trimString(ref.slot_id);
}

export function getPresentationRefSlotLabel(ref: PresentationMessageRef): string {
  const label = trimString(ref.label);
  if (label) return label;
  return slotLabelFromSlotId(getPresentationRefSlotId(ref));
}

export function getPresentationRefLocatorLabel(ref: PresentationMessageRef): string {
  const label = trimString(ref.locator_label);
  if (label) return label;
  return defaultLocatorLabel(trimString(ref.card_type));
}

export function getPresentationRefChipLabel(ref: PresentationMessageRef): string {
  const slotLabel = getPresentationRefSlotLabel(ref);
  const locatorLabel = getPresentationRefLocatorLabel(ref);
  return locatorLabel ? `${slotLabel} · ${locatorLabel}` : slotLabel;
}

type BuildPresentationRefOptions = {
  baseRef?: PresentationMessageRef | null;
  status?: PresentationRefStatus;
  locatorLabel?: string;
  href?: string;
  excerpt?: string;
  locator?: Record<string, unknown>;
  snapshot?: PresentationRefSnapshot;
};

export function buildPresentationRefForSlot(
  slot: PresentationSlot | null,
  options?: BuildPresentationRefOptions,
): PresentationMessageRef | null {
  if (!slot?.card) return null;
  const card: PresentationCard = slot.card;
  const baseRef =
    options?.baseRef && isPresentationMessageRef(options.baseRef) ? options.baseRef : null;
  const slotId = trimString(slot.slot_id);
  if (!slotId) return null;
  const baseLocator = asRecord(baseRef?.locator);
  const nextLocator = options?.locator
    ? { ...(baseLocator || {}), ...options.locator }
    : baseLocator || undefined;
  const href =
    trimString(options?.href) || trimString(baseRef?.href) || trimString(card.content.url);
  const locatorLabel =
    trimString(options?.locatorLabel) ||
    trimString(baseRef?.locator_label) ||
    defaultLocatorLabel(card.card_type);
  const excerpt = trimString(options?.excerpt) || trimString(baseRef?.excerpt);
  const status =
    options?.status ||
    getPresentationRefStatus(
      baseRef || ({ slot_id: slotId, kind: "presentation_ref" } as PresentationMessageRef),
    );
  const snapshot = options?.snapshot || baseRef?.snapshot;

  return {
    kind: "presentation_ref",
    v: 1,
    slot_id: slotId,
    label: baseRef?.label || `P${slot.index}`,
    locator_label: locatorLabel,
    title: trimString(baseRef?.title) || trimString(card.title),
    card_type: trimString(baseRef?.card_type) || trimString(card.card_type),
    status,
    href: href || undefined,
    excerpt: excerpt || undefined,
    locator: nextLocator,
    snapshot: snapshot || undefined,
  };
}
