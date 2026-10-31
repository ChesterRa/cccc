import { afterEach, expect, it, vi } from "vite-plus/test";
import { formatElapsed, formatTime } from "./time";

afterEach(() => vi.useRealTimers());

it("formats each existing relative threshold in the interface language", () => {
  vi.useFakeTimers();
  const now = new Date(2026, 9, 30, 12, 0, 0);
  vi.setSystemTime(now);
  const ago = (seconds: number) => new Date(now.getTime() - seconds * 1000).toISOString();
  expect(formatTime(ago(59), "en")).toBe("just now");
  expect(formatTime(ago(59), "zh")).toBe("刚刚");
  expect(formatTime(ago(59), "ja")).toBe("たった今");
  expect(formatTime(ago(60), "en")).toBe("1m ago");
  expect(formatTime(ago(60 * 59), "en")).toBe("59m ago");
  expect(formatTime(ago(60 * 60), "en")).toBe("1h ago");
  expect(formatTime(ago(60 * 60 * 23), "en")).toBe("23h ago");
  expect(formatTime(ago(60 * 60 * 24), "en")).toBe("1d ago");
  expect(formatTime(ago(60 * 60 * 24 * 6), "en")).toBe("6d ago");
  expect(formatTime(ago(60 * 5), "zh-CN")).toBe("5分钟前");
  expect(formatTime(ago(60 * 5), "ja")).toBe("5分前");
  expect(formatTime(ago(60 * 5), undefined)).toBe("5m ago");
});

it("localizes older dates while preserving invalid and missing values", () => {
  vi.useFakeTimers();
  const now = new Date(2026, 9, 30, 12, 0, 0);
  vi.setSystemTime(now);
  const recent = new Date(2026, 9, 23, 12, 0, 0).toISOString();
  const previousYear = new Date(2025, 9, 3, 12, 0, 0).toISOString();
  expect(formatTime(recent, "en")).toBe("Oct 23");
  expect(formatTime(recent, "zh")).toBe("10月23日");
  expect(formatTime(recent, "ja")).toBe("10月23日");
  expect(formatTime(previousYear, "en")).toBe("Oct 3, 2025");
  expect(formatTime(previousYear, "zh")).toBe("2025年10月3日");
  expect(formatTime(previousYear, "ja")).toBe("2025年10月3日");
  expect(formatTime(undefined, "zh")).toBe("—");
  expect(formatTime("not-a-date", "ja")).toBe("not-a-date");
});

it("uses the device's current time zone for older dates even after it changes", () => {
  const originalTimeZone = process.env.TZ;
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-20T12:00:00Z"));
  const iso = "2026-03-30T23:00:00Z";
  try {
    process.env.TZ = "UTC";
    expect(formatTime(iso, "en")).toBe("Mar 30");
    process.env.TZ = "Asia/Tokyo";
    expect(formatTime(iso, "en")).toBe("Mar 31");
    expect(formatTime(iso, "ja")).toBe("3月31日");
  } finally {
    if (originalTimeZone === undefined) delete process.env.TZ;
    else process.env.TZ = originalTimeZone;
  }
});

it("matches the device's local calendar for different zones and years", () => {
  const originalTimeZone = process.env.TZ;
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-20T12:00:00Z"));
  try {
    for (const zone of ["UTC", "Asia/Tokyo", "America/Los_Angeles", "Pacific/Kiritimati"]) {
      process.env.TZ = zone;
      for (const iso of ["2025-12-31T23:30:00Z", "2026-03-08T09:30:00Z", "2026-11-01T08:30:00Z"]) {
        const date = new Date(iso);
        const withYear = date.getFullYear() !== new Date().getFullYear();
        for (const locale of ["en", "zh", "ja"]) {
          const expected = date.toLocaleDateString(locale, {
            month: "short",
            day: "numeric",
            ...(withYear ? { year: "numeric" } : {}),
          });
          expect(formatTime(iso, locale)).toBe(expected);
        }
      }
    }
  } finally {
    if (originalTimeZone === undefined) delete process.env.TZ;
    else process.env.TZ = originalTimeZone;
  }
});

it("names elapsed seconds in their largest whole unit and the page's language", () => {
  expect(formatElapsed(59, "en")).toBe("59 seconds ago");
  expect(formatElapsed(60, "en")).toBe("1 minute ago");
  expect(formatElapsed(3599, "en")).toBe("59 minutes ago");
  expect(formatElapsed(3600, "en")).toBe("1 hour ago");
  expect(formatElapsed(86399, "en")).toBe("23 hours ago");
  expect(formatElapsed(86400 * 3, "en-US")).toBe("3 days ago");
  expect(formatElapsed(7200, "zh-CN")).toBe("2小时前");
  expect(formatElapsed(7200, "ja")).toBe("2 時間前");
  expect(formatElapsed(7200, undefined)).toBe("2 hours ago");
});
