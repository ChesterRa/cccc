import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import i18n, { i18nReady } from "../i18n";
import { formatDaemonError } from "./daemonErrorText";

const MAPPED_CODES = [
  "delivery_in_progress",
  "delivery_ambiguous",
  "already_delivered",
  "shutdown_in_progress",
] as const;

describe("formatDaemonError", () => {
  beforeEach(async () => {
    await i18nReady;
    await i18n.changeLanguage("en");
  });

  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it.each(["en", "zh", "ja"] as const)(
    "maps the four actionable codes with original code in parentheses for %s",
    async (lang) => {
      await i18n.changeLanguage(lang);
      for (const code of MAPPED_CODES) {
        const formatted = formatDaemonError({ code, message: "raw daemon text" });
        expect(formatted).not.toContain("raw daemon text");
        expect(formatted).not.toContain("daemonErrors.");
        expect(formatted).toContain(lang === "en" ? `(${code})` : `（${code}）`);
      }
    },
  );

  it("keeps raw code and message for unmapped errors and handles partial shapes", () => {
    expect(formatDaemonError({ code: "actor_not_found", message: "Actor missing" })).toBe(
      "actor_not_found: Actor missing",
    );
    expect(formatDaemonError({ code: "actor_not_found", message: "" })).toBe("actor_not_found");
    expect(formatDaemonError({ code: "", message: "Network failed" })).toBe("Network failed");
    expect(formatDaemonError(null)).toBe("");
    expect(formatDaemonError(undefined)).toBe("");
  });
});
