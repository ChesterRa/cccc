import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import i18n, { i18nReady } from "../i18n";
import { formatDaemonError } from "./daemonErrorText";

const MAPPED_CODES = [
  "delivery_in_progress",
  "delivery_ambiguous",
  "already_delivered",
  "shutdown_in_progress",
  "runtime_shutting_down",
  "daemon_unavailable",
  "path_not_found",
  "not_dir",
  "invalid_project_root",
  "no_enabled_recipients",
  "file_too_large",
  "actor_browser_busy",
  "actor_switch_pending",
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
    "maps every actionable daemon code with original code in parentheses for %s",
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

  it("reuses shutdown guidance for runtime_shutting_down while preserving its own code", async () => {
    await i18n.changeLanguage("zh");
    const shutdownText = formatDaemonError({ code: "shutdown_in_progress", message: "raw" });
    const runtimeText = formatDaemonError({ code: "runtime_shutting_down", message: "raw" });
    expect(runtimeText).toBe(shutdownText.replace("shutdown_in_progress", "runtime_shutting_down"));
  });

  it("keeps raw code and message for unmapped and diagnostic errors and handles partial shapes", () => {
    for (const code of [
      "actor_not_found",
      "invalid_args",
      "invalid_path",
      "setup_required",
      "runtime_error",
      "runtime_mcp_setup_failed",
    ]) {
      expect(formatDaemonError({ code, message: "Detailed diagnostic text" })).toBe(
        `${code}: Detailed diagnostic text`,
      );
    }
    expect(formatDaemonError({ code: "actor_not_found", message: "" })).toBe("actor_not_found");
    expect(formatDaemonError({ code: "", message: "Network failed" })).toBe("Network failed");
    expect(formatDaemonError(null)).toBe("");
    expect(formatDaemonError(undefined)).toBe("");
  });
});
