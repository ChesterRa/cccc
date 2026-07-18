import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { formatMessageTimestamp } from "../../src/utils/time";

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function expectedMessageTimestamp(date: Date, now: Date): string {
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  if (sameDay) {
    return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  }
  const monthDayTime = `${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  if (date.getFullYear() === now.getFullYear()) {
    return monthDayTime;
  }
  return `${date.getFullYear()}-${monthDayTime}`;
}

describe("formatMessageTimestamp", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("formats messages from today as hour and minute", () => {
    const now = new Date("2026-03-23T12:34:56Z");
    vi.setSystemTime(now);
    const value = new Date(now.getTime() - 90 * 60 * 1000);
    expect(formatMessageTimestamp(value.toISOString())).toBe(expectedMessageTimestamp(value, now));
  });

  it("formats messages from the same year with month, day, and time", () => {
    const now = new Date("2026-03-23T12:34:56Z");
    vi.setSystemTime(now);
    const value = new Date(now.getTime() - 5 * 24 * 60 * 60 * 1000);
    expect(formatMessageTimestamp(value.toISOString())).toBe(expectedMessageTimestamp(value, now));
  });

  it("formats messages from prior years with full date and time", () => {
    const now = new Date("2026-03-23T12:34:56Z");
    vi.setSystemTime(now);
    const value = new Date("2025-11-18T08:09:00Z");
    expect(formatMessageTimestamp(value.toISOString())).toBe(expectedMessageTimestamp(value, now));
  });

  it("passes through invalid timestamps", () => {
    expect(formatMessageTimestamp("not-a-date")).toBe("not-a-date");
    expect(formatMessageTimestamp(undefined)).toBe("—");
  });
});
