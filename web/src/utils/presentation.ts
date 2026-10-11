import type { GroupPresentation, PresentationSlot } from "../types";
import { isLocalWebHostname } from "./webAddress";

function buildEmptySlot(index: number): PresentationSlot {
  return { slot_id: `slot-${index}`, index, card: null };
}

export function ensurePresentation(
  presentation: GroupPresentation | null | undefined,
): GroupPresentation {
  const slots = Array.isArray(presentation?.slots) ? presentation.slots : [];
  const slotsById = new Map<string, PresentationSlot>();
  for (const slot of slots) {
    const normalizedId = String(slot?.slot_id || "").trim();
    if (!normalizedId) continue;
    slotsById.set(normalizedId, slot);
  }

  return {
    v: Number(presentation?.v || 1) || 1,
    updated_at: String(presentation?.updated_at || "").trim(),
    highlight_slot_id: String(presentation?.highlight_slot_id || "").trim(),
    slots: Array.from({ length: 4 }, (_, index) => {
      const slotId = `slot-${index + 1}`;
      return slotsById.get(slotId) || buildEmptySlot(index + 1);
    }),
  };
}

export function findPresentationSlot(
  presentation: GroupPresentation | null | undefined,
  slotId: string,
): PresentationSlot | null {
  const normalizedSlotId = String(slotId || "").trim();
  if (!normalizedSlotId) return null;
  return (
    ensurePresentation(presentation).slots.find((slot) => slot.slot_id === normalizedSlotId) || null
  );
}

function isIpv4Hostname(hostname: string): boolean {
  const parts = hostname.split(".").map((part) => Number(part));
  return (
    parts.length === 4 && parts.every((part) => Number.isInteger(part) && part >= 0 && part <= 255)
  );
}

function hostCandidateFromUrlLikeInput(raw: string): string {
  const head = String(raw || "").split(/[/?#]/, 1)[0] || "";
  if (head.startsWith("[")) {
    const end = head.indexOf("]");
    if (end >= 0) return head.slice(0, end + 1);
  }
  const colonIndex = head.indexOf(":");
  return colonIndex >= 0 ? head.slice(0, colonIndex) : head;
}

function isLikelyPresentationUrlInput(raw: string): boolean {
  const trimmed = String(raw || "").trim();
  if (!trimmed) return false;
  if (/\s/.test(trimmed)) return false;
  if (trimmed.startsWith("/") || trimmed.startsWith("./") || trimmed.startsWith("../"))
    return false;
  if (trimmed.startsWith("//")) return true;
  const hostCandidate = hostCandidateFromUrlLikeInput(trimmed).toLowerCase();
  if (!hostCandidate) return false;
  if (hostCandidate === "localhost" || hostCandidate === "host.docker.internal") return true;
  if (hostCandidate === "[::1]" || hostCandidate.endsWith(".local")) return true;
  if (hostCandidate.startsWith("www.")) return true;
  if (isIpv4Hostname(hostCandidate)) return true;
  return /^[a-z0-9-]+(?:\.[a-z0-9-]+)+$/i.test(hostCandidate);
}

function shouldDefaultPresentationUrlToHttp(raw: string): boolean {
  const hostCandidate = hostCandidateFromUrlLikeInput(raw).toLowerCase();
  if (!hostCandidate) return false;
  if (
    hostCandidate === "localhost" ||
    hostCandidate === "host.docker.internal" ||
    hostCandidate === "[::1]"
  ) {
    return true;
  }
  if (hostCandidate.endsWith(".local")) return true;
  if (isIpv4Hostname(hostCandidate)) return true;
  return false;
}

export function normalizePresentationUrlInput(url: string): string {
  const raw = String(url || "").trim();
  if (!raw) return "";
  if (/^https?:\/\//i.test(raw)) return raw;
  if (!isLikelyPresentationUrlInput(raw)) return raw;
  if (raw.startsWith("//")) return `https:${raw}`;
  const prefix = shouldDefaultPresentationUrlToHttp(raw) ? "http://" : "https://";
  const candidate = `${prefix}${raw}`;
  try {
    const parsed = new URL(candidate);
    const protocol = String(parsed.protocol || "")
      .trim()
      .toLowerCase();
    if ((protocol === "http:" || protocol === "https:") && String(parsed.hostname || "").trim()) {
      return candidate;
    }
  } catch {
    // Fall through and let callers surface a friendly validation error.
  }
  return raw;
}

export function isValidPresentationWebUrl(url: string): boolean {
  const raw = String(url || "").trim();
  if (!raw) return false;
  try {
    const parsed = new URL(raw);
    const protocol = String(parsed.protocol || "")
      .trim()
      .toLowerCase();
    return (
      (protocol === "http:" || protocol === "https:") && !!String(parsed.hostname || "").trim()
    );
  } catch {
    return false;
  }
}

export function shouldPreferPresentationLiveBrowser(url: string): boolean {
  const raw = normalizePresentationUrlInput(url);
  if (!raw) return false;
  try {
    const parsed = new URL(raw);
    const protocol = String(parsed.protocol || "")
      .trim()
      .toLowerCase();
    if (protocol !== "http:" && protocol !== "https:") return false;
    const hostname = String(parsed.hostname || "")
      .trim()
      .toLowerCase();
    if (!hostname) return false;
    return isLocalWebHostname(hostname);
  } catch {
    return false;
  }
}
