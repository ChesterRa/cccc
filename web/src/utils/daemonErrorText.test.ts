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
  "group_copy_invalid_package",
] as const;

const LIMIT_MAPPED_CODES = [
  { code: "avatar_too_large", maxBytes: 2 * 1024 * 1024, expectedLimit: "2 MiB" },
  { code: "branding_asset_too_large", maxBytes: 2 * 1024 * 1024, expectedLimit: "2 MiB" },
  { code: "group_copy_too_large", maxBytes: 1024 * 1024 * 1024, expectedLimit: "1 GiB" },
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

  it.each(["en", "zh", "ja"] as const)(
    "formats upload too-large guidance from details.max_bytes for %s",
    async (lang) => {
      await i18n.changeLanguage(lang);
      for (const { code, maxBytes, expectedLimit } of LIMIT_MAPPED_CODES) {
        const formatted = formatDaemonError({
          code,
          message: "raw daemon text",
          details: { max_bytes: maxBytes },
        });
        expect(formatted).not.toContain("raw daemon text");
        expect(formatted).not.toContain("{{limit}}");
        expect(formatted).toContain(expectedLimit);
        expect(formatted).toContain(lang === "en" ? `(${code})` : `（${code}）`);
      }
    },
  );

  it("falls back to raw diagnostic when max_bytes is missing or invalid", async () => {
    await i18n.changeLanguage("zh");
    for (const { code } of LIMIT_MAPPED_CODES) {
      expect(formatDaemonError({ code, message: "raw limit text" })).toBe(
        `${code}: raw limit text`,
      );
      expect(
        formatDaemonError({ code, message: "raw limit text", details: { max_bytes: 0 } }),
      ).toBe(`${code}: raw limit text`);
      expect(
        formatDaemonError({ code, message: "raw limit text", details: { max_bytes: "2 MiB" } }),
      ).toBe(`${code}: raw limit text`);
    }
  });

  it.each(["en", "zh", "ja"] as const)(
    "names the accepted branding image types from details.accepted for %s",
    async (lang) => {
      await i18n.changeLanguage(lang);
      const code = "branding_asset_unsupported_type";
      const formatted = formatDaemonError({
        code,
        message: "unsupported favicon type: text/plain",
        details: { accepted: ["PNG", "SVG", "ICO"] },
      });
      expect(formatted).not.toContain("text/plain");
      expect(formatted).not.toContain("{{types}}");
      expect(formatted).toContain("PNG, SVG, ICO");
      expect(formatted).toContain(lang === "en" ? `(${code})` : `（${code}）`);
    },
  );

  it("falls back to the raw message when the accepted types are missing", async () => {
    await i18n.changeLanguage("zh");
    const code = "branding_asset_unsupported_type";
    expect(formatDaemonError({ code, message: "raw type text" }, "failed")).toBe("raw type text");
    expect(formatDaemonError({ code, message: "raw type text", details: { accepted: [] } })).toBe(
      `${code}: raw type text`,
    );
  });

  it("preserves raw message or fallback for unmapped errors when a fallback is supplied", async () => {
    await i18n.changeLanguage("zh");
    expect(
      formatDaemonError({ code: "invalid_request", message: "unsupported favicon type" }, "failed"),
    ).toBe("unsupported favicon type");
    expect(formatDaemonError({ code: "invalid_request", message: "" }, "failed")).toBe("failed");
    expect(formatDaemonError(undefined, "failed")).toBe("failed");
    expect(
      formatDaemonError(
        {
          code: "avatar_too_large",
          message: "avatar file exceeds 2 MiB",
          details: { max_bytes: 2 * 1024 * 1024 },
        },
        "failed",
      ),
    ).toContain("（avatar_too_large）");
    expect(
      formatDaemonError(
        { code: "avatar_too_large", message: "avatar file exceeds 2 MiB" },
        "failed",
      ),
    ).toBe("avatar file exceeds 2 MiB");
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
