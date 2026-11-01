/** Host refusals in the user's language; anything else keeps the host's detail. */
export function chetRefusalText(
  t: (key: string, options?: Record<string, unknown>) => string,
  code: string,
  message: string,
): string {
  if (code === "codex_voice_recovery_pending") return t("chet.errors.recoveryPending");
  if (code === "codex_voice_restarting") return t("chet.errors.restarting");
  if (code === "read_only") return t("chet.errors.readOnly");
  if (["unauthorized", "auth_required", "admin_required", "permission_denied"].includes(code)) {
    return t("chet.errors.permission");
  }
  return t("chet.errors.invalid", { detail: message || code });
}
