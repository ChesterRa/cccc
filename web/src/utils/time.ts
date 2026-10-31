import { normalizeLanguageCode, type LanguageCode } from "../i18n/languages";

const JUST_NOW: Record<LanguageCode, string> = { en: "just now", zh: "刚刚", ja: "たった今" };
const relativeFormats = new Map<string, Intl.RelativeTimeFormat>();
const dateFormats = new Map<string, Intl.DateTimeFormat>();

function relativeFormatter(locale: LanguageCode, style: "narrow" | "long") {
  const key = `${locale}:${style}`;
  let formatter = relativeFormats.get(key);
  if (!formatter) {
    formatter = new Intl.RelativeTimeFormat(locale, { style, numeric: "always" });
    relativeFormats.set(key, formatter);
  }
  return formatter;
}

function localDateFormatter(locale: LanguageCode, withYear: boolean) {
  const key = `${locale}:${withYear}`;
  let formatter = dateFormats.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, {
      timeZone: "UTC",
      month: "short",
      day: "numeric",
      ...(withYear ? { year: "numeric" } : {}),
    });
    dateFormats.set(key, formatter);
  }
  return formatter;
}

// DateTimeFormat captures the device time zone on construction. Format the
// current local calendar day in UTC instead, so cached instances still follow
// a device time zone change and daylight-saving offsets for each timestamp.
function localCalendarDay(date: Date): Date {
  const day = new Date(0);
  day.setUTCFullYear(date.getFullYear(), date.getMonth(), date.getDate());
  return day;
}

// Format an ISO timestamp in the interface language, retaining the existing
// relative/absolute time boundaries and the device's local time zone.
export function formatTime(isoStr: string | undefined, language: string | undefined): string {
  if (!isoStr) return "—";
  try {
    const date = new Date(isoStr);
    if (isNaN(date.getTime())) return isoStr;
    const now = new Date();
    const diffSec = Math.floor((now.getTime() - date.getTime()) / 1000);
    const diffMin = Math.floor(diffSec / 60);
    const diffHour = Math.floor(diffMin / 60);
    const diffDay = Math.floor(diffHour / 24);
    const locale = normalizeLanguageCode(language);
    if (diffSec < 60) {
      return JUST_NOW[locale];
    }
    if (diffMin < 60) return relativeFormatter(locale, "narrow").format(-diffMin, "minute");
    if (diffHour < 24) return relativeFormatter(locale, "narrow").format(-diffHour, "hour");
    if (diffDay < 7) return relativeFormatter(locale, "narrow").format(-diffDay, "day");
    return localDateFormatter(locale, date.getFullYear() !== now.getFullYear()).format(
      localCalendarDay(date),
    );
  } catch {
    return isoStr;
  }
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function isSameLocalDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function formatLocalMonthDayTime(date: Date): string {
  return `${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

export function formatMessageTimestamp(isoStr: string | undefined): string {
  if (!isoStr) return "—";
  try {
    const date = new Date(isoStr);
    if (isNaN(date.getTime())) return isoStr;
    const now = new Date();
    if (isSameLocalDay(date, now)) {
      return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
    }
    if (date.getFullYear() === now.getFullYear()) {
      return formatLocalMonthDayTime(date);
    }
    return `${date.getFullYear()}-${formatLocalMonthDayTime(date)}`;
  } catch {
    return isoStr;
  }
}

export function formatFullTime(isoStr: string | undefined): string {
  if (!isoStr) return "";
  try {
    const date = new Date(isoStr);
    if (isNaN(date.getTime())) return isoStr;
    return date.toLocaleString();
  } catch {
    return isoStr;
  }
}

/** Elapsed seconds in the page's language, in the largest whole unit: "2 hours ago". */
export function formatElapsed(seconds: number, language: string | undefined): string {
  const [value, unit]: [number, Intl.RelativeTimeFormatUnit] =
    seconds >= 86400
      ? [Math.floor(seconds / 86400), "day"]
      : seconds >= 3600
        ? [Math.floor(seconds / 3600), "hour"]
        : seconds >= 60
          ? [Math.floor(seconds / 60), "minute"]
          : [seconds, "second"];
  return relativeFormatter(normalizeLanguageCode(language), "long").format(-value, unit);
}
