import i18n from "i18next";

type DaemonErrorShape = { code?: unknown; message?: unknown; details?: unknown };

const DAEMON_ERROR_KEY_BY_CODE: Record<string, string> = {
  delivery_in_progress: "common:daemonErrors.delivery_in_progress",
  delivery_ambiguous: "common:daemonErrors.delivery_ambiguous",
  already_delivered: "common:daemonErrors.already_delivered",
  shutdown_in_progress: "common:daemonErrors.shutdown_in_progress",
  runtime_shutting_down: "common:daemonErrors.shutdown_in_progress",
  daemon_unavailable: "common:daemonErrors.daemon_unavailable",
  path_not_found: "common:daemonErrors.path_not_found",
  not_dir: "common:daemonErrors.not_dir",
  invalid_project_root: "common:daemonErrors.invalid_project_root",
  no_enabled_recipients: "common:daemonErrors.no_enabled_recipients",
  file_too_large: "common:daemonErrors.file_too_large",
  avatar_too_large: "common:daemonErrors.avatar_too_large",
  branding_asset_too_large: "common:daemonErrors.branding_asset_too_large",
  group_copy_too_large: "common:daemonErrors.group_copy_too_large",
  actor_browser_busy: "common:daemonErrors.actor_browser_busy",
  actor_switch_pending: "common:daemonErrors.actor_switch_pending",
};

const LIMIT_REQUIRED_CODES = new Set([
  "avatar_too_large",
  "branding_asset_too_large",
  "group_copy_too_large",
]);

function asString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

function formatByteLimit(value: unknown): string {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return "";
  const mib = 1024 * 1024;
  const gib = 1024 * mib;
  if (value % gib === 0) return `${value / gib} GiB`;
  if (value % mib === 0) return `${value / mib} MiB`;
  if (value % 1024 === 0) return `${value / 1024} KiB`;
  return `${Math.trunc(value)} bytes`;
}

function readMaxBytesLimit(details: unknown): string {
  if (!details || typeof details !== "object") return "";
  return formatByteLimit((details as { max_bytes?: unknown }).max_bytes);
}

/**
 * Formats daemon API errors for user-facing banners, mapping common actionable
 * codes to localized guidance with the original code in parentheses.
 */
export function formatDaemonError(
  error: DaemonErrorShape | null | undefined,
  fallback?: string,
): string {
  const code = asString(error?.code).trim();
  const message = asString(error?.message).trim();
  const key = Object.prototype.hasOwnProperty.call(DAEMON_ERROR_KEY_BY_CODE, code)
    ? DAEMON_ERROR_KEY_BY_CODE[code]
    : "";
  if (key && i18n.isInitialized && i18n.exists(key)) {
    if (LIMIT_REQUIRED_CODES.has(code)) {
      const limit = readMaxBytesLimit(error?.details);
      if (limit) {
        return i18n.t(key, { code, limit });
      }
    } else {
      return i18n.t(key, { code });
    }
  }
  if (fallback !== undefined) {
    return message || fallback;
  }
  if (code && message) return `${code}: ${message}`;
  return message || code;
}
