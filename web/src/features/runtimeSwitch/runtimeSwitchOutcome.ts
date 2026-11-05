type SwitchError = { code: string; message: string; details?: unknown };

/** Join translated sentences without inserting spaces after CJK punctuation. */
export function joinRuntimeSentences(parts: Array<string | null | undefined | false>): string {
  return parts.reduce<string>((joined, part) => {
    if (!part) return joined;
    if (!joined) return part;
    return `${joined}${/[。！？]$/.test(joined) ? "" : " "}${part}`;
  }, "");
}

export function runtimeSwitchStartError(
  error: unknown,
  translate: (key: string) => string,
): string {
  const message =
    error && typeof error === "object" && "message" in error && typeof error.message === "string"
      ? error.message.trim()
      : "";
  return joinRuntimeSentences([translate("runtimeSwitch.startError"), message]);
}

export function runtimeSwitchErrorMessage(
  error: SwitchError,
  translate: (key: string) => string,
): string {
  if (error.code === "actor_switch_unknown") return translate("runtimeSwitch.unknown");
  if (error.code === "actor_switch_conflict") return translate("runtimeSwitch.conflict");
  if (error.code === "actor_switch_pending") return translate("runtimeSwitch.previousPending");
  if (error.code === "actor_switch_incomplete") {
    const details =
      error.details && typeof error.details === "object"
        ? (error.details as Record<string, unknown>)
        : {};
    const key =
      {
        native: "runtimeSwitch.incompleteNative",
        not_running: "runtimeSwitch.incompleteStopped",
        restarted: "runtimeSwitch.incompleteRestarted",
        restart_failed: "runtimeSwitch.incompleteRestartFailed",
        unknown: "runtimeSwitch.incompleteUnknown",
      }[String(details.old_owner)] || "runtimeSwitch.failed";
    return joinRuntimeSentences([translate(key), error.message]);
  }
  return joinRuntimeSentences([translate("runtimeSwitch.failed"), error.message]);
}
