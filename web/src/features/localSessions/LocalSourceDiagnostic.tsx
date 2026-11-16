import { useTranslation } from "react-i18next";
import type { SourceState } from "../../services/api/localSessionControls";

export function LocalSourceDiagnostic({
  state,
  supported,
}: {
  state?: SourceState;
  supported: boolean;
}) {
  const { t, i18n } = useTranslation("layout");
  const reason = state?.diagnostic.state || "unknown";
  const known = [
    "ready",
    "shared_backend_missing",
    "thread_not_loaded",
    "receive_check_failed",
    "unknown",
  ].includes(reason)
    ? reason
    : "unknown";
  if (!supported) return null;
  if (state?.control.supported === false)
    return (
      <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
        {t(
          state.control.reason_code === "remote_source_control_unsupported"
            ? "sourceControls.remoteUnsupported"
            : "sourceControls.unsupported",
        )}
      </p>
    );
  const timestamp = state?.diagnostic.checked_at;
  const checked = timestamp && Number.isFinite(Date.parse(timestamp));
  return (
    <div className="min-w-0 space-y-1 text-xs leading-relaxed text-[var(--color-text-secondary)]">
      <p role="status">
        {t(
          state?.control.access_blocked
            ? state.control.pause_complete
              ? "sourceControls.blocked"
              : "sourceControls.partial"
            : `sourceControls.reasons.${known}`,
        )}
      </p>
      {!state?.control.access_blocked && known !== "ready" && (
        <p>{t(`sourceControls.guidance.${known}`)}</p>
      )}
      {checked && (
        <p>
          {t("sourceControls.checkedAt", {
            time: new Date(timestamp).toLocaleTimeString(i18n.language),
          })}
        </p>
      )}
    </div>
  );
}
