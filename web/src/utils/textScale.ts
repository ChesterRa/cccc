import type { TextScale } from "../types";

export const TEXT_SCALE_STORAGE_KEY = "cccc-ui-text-scale";
export const TEXT_SCALE_OPTIONS: TextScale[] = [90, 100, 125];
export const DEFAULT_TEXT_SCALE: TextScale = 100;

export function normalizeTextScale(value: unknown): TextScale {
  const numeric = Number(value);
  if (TEXT_SCALE_OPTIONS.includes(numeric as TextScale)) {
    return numeric as TextScale;
  }
  return DEFAULT_TEXT_SCALE;
}

export function getStoredTextScale(): TextScale {
  if (typeof window === "undefined") return DEFAULT_TEXT_SCALE;
  return normalizeTextScale(window.localStorage.getItem(TEXT_SCALE_STORAGE_KEY));
}

export function getTextScaleLabel(scale: TextScale): string {
  return `${normalizeTextScale(scale)}%`;
}

export function getNextTextScale(scale: TextScale): TextScale {
  const normalized = normalizeTextScale(scale);
  const currentIndex = TEXT_SCALE_OPTIONS.indexOf(normalized);
  const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % TEXT_SCALE_OPTIONS.length : 0;
  return TEXT_SCALE_OPTIONS[nextIndex];
}

export function applyTextScale(scale: TextScale): TextScale {
  const normalized = normalizeTextScale(scale);
  if (typeof document !== "undefined") {
    document.documentElement.style.fontSize = `${normalized}%`;
  }
  return normalized;
}
