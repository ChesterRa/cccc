import i18n from "i18next";

type DaemonErrorShape = { code?: unknown; message?: unknown };

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
  actor_browser_busy: "common:daemonErrors.actor_browser_busy",
  actor_switch_pending: "common:daemonErrors.actor_switch_pending",
};

function asString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

/**
 * Formats daemon API errors for user-facing banners, mapping common actionable
 * codes to localized guidance with the original code in parentheses.
 */
export function formatDaemonError(error: DaemonErrorShape | null | undefined): string {
  const code = asString(error?.code).trim();
  const message = asString(error?.message).trim();
  const key = Object.prototype.hasOwnProperty.call(DAEMON_ERROR_KEY_BY_CODE, code)
    ? DAEMON_ERROR_KEY_BY_CODE[code]
    : "";
  if (key && i18n.isInitialized && i18n.exists(key)) {
    return i18n.t(key, { code });
  }
  if (code && message) return `${code}: ${message}`;
  return message || code;
}
